//! 仓库层：所有 SQL 都在这里 —— 阶段 4
//!
//! 对应 docs/03-数据库设计.md。命令层（lib.rs）只调这里，不直接写 SQL。

use super::db::DbError;
use super::detect;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// 与 api.ts 的 ClipboardItem 对齐
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardItem {
    pub id: i64,
    pub content: String,
    pub content_type: String,
    pub preview: String,
    pub image_path: Option<String>,
    pub byte_size: i64,
    pub copy_count: i64,
    pub source_app: Option<String>,
    pub created_at: i64,
    pub last_copied_at: i64,
    pub favorite: bool,
    pub sensitive: bool,
    /// 到期自动清除（毫秒）。None = 永不过期。
    /// 目前只有敏感项会带，前端暂不展示，只随 JSON 带出去
    pub expires_at: Option<i64>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Query {
    pub text: Option<String>,
    pub types: Option<Vec<String>>,
    pub favorite_only: Option<bool>,
    /// 含敏感信息的分组。默认关 —— docs/03：敏感项不参与普通结果，
    /// 用户主动展开才可见。这层过滤在 SQL 里做，前端不碰
    pub include_sensitive: Option<bool>,
    pub since: Option<i64>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// 入库前的原始内容
#[derive(Debug, Clone)]
pub struct NewItem {
    pub content: String,
    pub source_app: Option<String>,
    pub image_path: Option<String>,
    /// 疑似密钥/token。由调用方先跑 sensitive::scan 再传进来 ——
    /// 仓库层不认规则，只认结论
    pub sensitive: bool,
    /// 到期自动清除（毫秒），None 表示不过期。
    /// 目前只有敏感项会带：docs/03 定的 now + 60s
    pub expires_at: Option<i64>,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("系统时间早于 Unix epoch")
        .as_millis() as i64
}

/// blake3(content) 的十六进制形式，去重依据
pub fn hash(content: &str) -> String {
    blake3::hash(content.as_bytes()).to_hex().to_string()
}

fn preview(content: &str, max: usize) -> String {
    let flat: String = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        let head: String = flat.chars().take(max).collect();
        format!("{head}…")
    }
}

const COLS: &str = "id, content, content_type, preview, image_path, byte_size,
     copy_count, source_app, created_at, last_copied_at, favorite, sensitive, expires_at";

fn row_to_item(r: &rusqlite::Row<'_>) -> rusqlite::Result<ClipboardItem> {
    Ok(ClipboardItem {
        id: r.get(0)?,
        content: r.get(1)?,
        content_type: r.get(2)?,
        preview: r.get(3)?,
        image_path: r.get(4)?,
        byte_size: r.get(5)?,
        copy_count: r.get(6)?,
        source_app: r.get(7)?,
        created_at: r.get(8)?,
        last_copied_at: r.get(9)?,
        favorite: r.get::<_, i64>(10)? != 0,
        sensitive: r.get::<_, i64>(11)? != 0,
        expires_at: r.get(12)?,
    })
}

/// 入库。内容已存在则累加 copy_count 并刷新时间，不新增行。
/// 返回 (id, 是否新建)。
pub fn upsert(conn: &Connection, item: &NewItem) -> Result<(i64, bool), DbError> {
    let h = hash(&item.content);
    let now = now_ms();
    let ctype = detect::detect(&item.content).as_str();
    let bytes = item.content.len() as i64;

    // 去重：hash 命中就只累加，不插入。并发下靠 UNIQUE 约束兜底
    let existing: Option<(i64, i64)> = conn
        .query_row(
            "SELECT id, copy_count FROM clipboard_item WHERE content_hash = ?1",
            params![h],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;

    if let Some((id, count)) = existing {
        // 敏感项被再次复制说明用户还在用它，到期时间要顺延，
        // 否则粘到一半条目就从历史里消失了。
        // 非敏感项 expires_at 本来就是 NULL，顺延是空操作
        let expiry_sql = if item.expires_at.is_some() {
            ", expires_at = ?4"
        } else {
            ""
        };
        conn.execute(
            &format!(
                "UPDATE clipboard_item
                 SET copy_count = ?2, last_copied_at = ?3{expiry_sql},
                     source_app = COALESCE(?5, source_app)
                 WHERE id = ?1"
            ),
            params![id, count + 1, now, item.expires_at, item.source_app],
        )?;
        return Ok((id, false));
    }

    let pv = preview(&item.content, 200);
    conn.execute(
        "INSERT INTO clipboard_item
           (content, content_hash, content_type, preview, image_path, byte_size,
            copy_count, source_app, created_at, last_copied_at, sensitive, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8, ?8, ?9, ?10)",
        params![
            item.content,
            h,
            ctype,
            pv,
            item.image_path,
            bytes,
            item.source_app,
            now,
            item.sensitive,
            item.expires_at
        ],
    )?;
    Ok((conn.last_insert_rowid(), true))
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<ClipboardItem>, DbError> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLS} FROM clipboard_item WHERE id = ?1"),
            params![id],
            row_to_item,
        )
        .optional()?)
}

pub fn list(conn: &Connection, q: &Query) -> Result<Vec<ClipboardItem>, DbError> {
    let limit = q.limit.unwrap_or(200).clamp(1, 1000);
    let offset = q.offset.unwrap_or(0).max(0);

    // 有搜索词走 FTS5，其余走主表。两套 SQL 是 trigram 索引的固有代价：
    // 外部内容表没法和主表做普通的 WHERE 合并，只能 JOIN
    let mut sql = String::new();
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    let term = q.text.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty());

    match term {
        Some(text) if text.chars().count() >= TRIGRAM_MIN => {
            sql.push_str(&format!(
                "SELECT {} FROM clipboard_item c JOIN item_fts f ON f.rowid = c.id WHERE item_fts MATCH ?{}",
                COLS.split(", ").map(|c| format!("c.{c}")).collect::<Vec<_>>().join(", "),
                args.len() + 1
            ));
            // 关键：MATCH 的查询串必须转义。用户输入的 " * - 等在 FTS5 里是
            // 语法字符，不转义会直接报 SQL 错误。整体加双引号当短语匹配
            args.push(Box::new(fts_phrase(text)));
        }
        Some(text) => {
            // trigram 至少要 3 个字符，"中文" 这种两字词用它搜不到。
            // 中文里两字词极常见，所以短的走 LIKE 兜底。
            // LIKE 是全表扫描，但只在短查询词时触发，且有 limit 兜着
            sql.push_str(&format!(
                "SELECT {COLS} FROM clipboard_item c WHERE c.content LIKE ?{} ESCAPE '\\'",
                args.len() + 1
            ));
            args.push(Box::new(format!("%{}%", like_escape(text))));
        }
        None => {
            sql.push_str(&format!("SELECT {COLS} FROM clipboard_item c WHERE 1=1"));
        }
    }

    // 三个分支都产出了 WHERE，这里统一追加。放在类型过滤之前
    // 没有特别的原因，只是敏感排除是「分组」语义，排最前
    if !q.include_sensitive.unwrap_or(false) {
        sql.push_str(" AND c.sensitive = 0");
    }
    if q.favorite_only.unwrap_or(false) {
        sql.push_str(" AND c.favorite = 1");
    }
    if let Some(types) = q.types.as_ref().filter(|t| !t.is_empty()) {
        let start = args.len() + 1;
        let holes = (0..types.len())
            .map(|i| format!("?{}", start + i))
            .collect::<Vec<_>>();
        sql.push_str(&format!(" AND c.content_type IN ({})", holes.join(", ")));
        for t in types {
            args.push(Box::new(t.clone()));
        }
    }
    if let Some(since) = q.since {
        sql.push_str(&format!(" AND c.last_copied_at >= ?{}", args.len() + 1));
        args.push(Box::new(since));
    }

    // 纯按最近复制时间排序 —— 收藏不再置顶（真机反馈：置顶让列表
    // 看起来不像时间序），收藏只靠星标与 favorite_only 筛选表达。
    // id DESC 做平局裁决：同毫秒内插入的两条（快速连按）也要有
    // 确定顺序，不能赌 SQLite 的返回——72b8503 的同族教训。
    // 与 mock 后端行为一致
    sql.push_str(" ORDER BY c.last_copied_at DESC, c.id DESC");
    sql.push_str(&format!(
        " LIMIT ?{} OFFSET ?{}",
        args.len() + 1,
        args.len() + 2
    ));
    args.push(Box::new(limit));
    args.push(Box::new(offset));

    let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(refs.as_slice(), row_to_item)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// trigram 分词器的最小匹配长度（字符数）。
/// 低于这个长度的查询词用 FTS5 搜不到任何东西
const TRIGRAM_MIN: usize = 3;

/// 把用户输入转成 FTS5 的安全短语查询。
///
/// trigram 下 `"` 包裹的是短语，但内部的双引号仍需转义成 `""`，
/// 否则 `he"llo` 这类输入会让整条 SQL 报错 —— 报错是静默失败之外的
/// 最坏情况，用户什么都搜不到却看不到原因。
fn fts_phrase(text: &str) -> String {
    let escaped = text.replace('"', "\"\"");
    format!("\"{escaped}\"")
}

/// LIKE 模式下转义 `%` `_` `\`，配合 `ESCAPE '\'` 使用。
/// 不转义的话用户搜 "50%" 会匹配到几乎所有行
fn like_escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

pub fn toggle_favorite(conn: &Connection, id: i64) -> Result<bool, DbError> {
    let now = conn.execute(
        "UPDATE clipboard_item SET favorite = 1 - favorite WHERE id = ?1",
        params![id],
    )?;
    if now == 0 {
        return Ok(false);
    }
    let f: i64 = conn.query_row(
        "SELECT favorite FROM clipboard_item WHERE id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    Ok(f != 0)
}

/// 编辑原条目（docs/04 通用操作 Edit）：换内容、重跑类型识别、
/// 重算去重哈希与预览。FTS 索引由 UPDATE 触发器自动重建
/// （db.rs 的 `clipboard_item_au`），保存后立刻能搜到新内容。
///
/// `sensitive` / `expires_at` 由调用方对**新内容**重扫后传入 ——
/// 与 upsert 同一个分工：仓库层不认规则，只认结论。不改敏感标记
/// 直接放行的话，一段改出来的密钥会以「普通文本」的身份留在
/// 永久历史里。
///
/// 返回 `None` 表示该 id 已不存在（被删了）。改成与另一条相同的
/// 内容报 `Conflict`：历史里留两条一模一样的没有意义，用户真正
/// 要做的多半是删掉手里这条 —— UNIQUE 约束报出来的是裸 SQL 错，
/// 照着修比照着做快得多
pub fn update_content(
    conn: &Connection,
    id: i64,
    content: &str,
    sensitive: bool,
    expires_at: Option<i64>,
) -> Result<Option<ClipboardItem>, DbError> {
    let h = hash(content);
    let other: Option<i64> = conn
        .query_row(
            "SELECT id FROM clipboard_item WHERE content_hash = ?1",
            params![h],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(other) = other.filter(|&o| o != id) {
        return Err(DbError::Conflict(format!(
            "历史里已有相同内容（第 {other} 条），不能改成重复的"
        )));
    }

    let n = conn.execute(
        "UPDATE clipboard_item
         SET content = ?2, content_hash = ?3, content_type = ?4,
             preview = ?5, byte_size = ?6, sensitive = ?7, expires_at = ?8
         WHERE id = ?1",
        params![
            id,
            content,
            h,
            detect::detect(content).as_str(),
            preview(content, 200),
            content.len() as i64,
            sensitive,
            expires_at
        ],
    )?;
    if n == 0 {
        return Ok(None);
    }
    get(conn, id)
}

pub fn remove(conn: &Connection, ids: &[i64]) -> Result<(), DbError> {
    for id in ids {
        conn.execute("DELETE FROM clipboard_item WHERE id = ?1", params![id])?;
    }
    Ok(())
}

pub fn clear(conn: &Connection) -> Result<(), DbError> {
    conn.execute("DELETE FROM clipboard_item", [])?;
    Ok(())
}

pub fn count(conn: &Connection) -> Result<i64, DbError> {
    Ok(conn.query_row("SELECT count(*) FROM clipboard_item", [], |r| r.get(0))?)
}

/// 查已到期的条目。阶段 7 的后台清理任务会调用
pub fn expired(conn: &Connection, now: i64) -> Result<Vec<i64>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id FROM clipboard_item WHERE expires_at IS NOT NULL AND expires_at <= ?1",
    )?;
    let rows = stmt.query_map(params![now], |r| r.get::<_, i64>(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 删除全部已到期条目，返回被删的内容原文。
///
/// 调用方（后台任务）拿它去比对剪贴板：如果剪贴板里躺着的正是
/// 刚删掉的敏感内容，就得把它一并清掉 —— 不然条目没了，
/// 密钥还挂在系统剪贴板上等着下一个粘贴目标。
///
/// 取数与删除在同一把连接锁内完成，调用方不要在两次调用之间
/// 制造窗口让别的内容插进来
pub fn purge_expired(conn: &Connection, now: i64) -> Result<Vec<String>, DbError> {
    let ids = expired(conn, now)?;
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let mut contents = Vec::with_capacity(ids.len());
    for id in &ids {
        if let Some(it) = get(conn, *id)? {
            contents.push(it.content);
        }
    }
    remove(conn, &ids)?;
    Ok(contents)
}

/// 保留期与条数上限的清理。收藏项永不自动清理 —— 用户明确标记过的东西。
/// 阶段 7 接到设置项后由后台任务调用
#[allow(dead_code)]
pub fn prune(
    conn: &Connection,
    max_items: i64,
    retention_ms: Option<i64>,
) -> Result<usize, DbError> {
    let mut n = 0;
    if let Some(cut) = retention_ms {
        n += conn.execute(
            // `<=` 而不是 `<`：cutoff 边界上的那一毫秒也要算过期。
            // 否则 retention=0（「立即过期」）在两次 add 落在同一毫秒时
            // 一条都删不掉 —— 那正是剪贴板被快速连按时的常态
            "DELETE FROM clipboard_item
             WHERE favorite = 0 AND expires_at IS NULL
               AND created_at <= ?1 - ?2",
            params![now_ms(), cut],
        )?;
    }
    if max_items > 0 {
        n += conn.execute(
            "DELETE FROM clipboard_item WHERE favorite = 0 AND id NOT IN (
                 SELECT id FROM clipboard_item WHERE favorite = 0
                 ORDER BY last_copied_at DESC LIMIT ?1)",
            params![max_items],
        )?;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn add(conn: &Connection, s: &str) -> (i64, bool) {
        upsert(
            conn,
            &NewItem {
                content: s.into(),
                source_app: None,
                image_path: None,
                sensitive: false,
                expires_at: None,
            },
        )
        .unwrap()
    }

    fn add_sensitive(conn: &Connection, s: &str, ttl_ms: i64) -> (i64, bool) {
        upsert(
            conn,
            &NewItem {
                content: s.into(),
                source_app: None,
                image_path: None,
                sensitive: true,
                expires_at: Some(now_ms() + ttl_ms),
            },
        )
        .unwrap()
    }

    #[test]
    fn upsert_inserts_then_accumulates() {
        let c = open_in_memory().unwrap();
        let (id1, new1) = add(&c, "hello");
        let (id2, new2) = add(&c, "hello");
        assert!(new1 && !new2, "第二次应命中去重");
        assert_eq!(id1, id2, "同内容应复用同一行");
        assert_eq!(count(&c).unwrap(), 1);
        let it = get(&c, id1).unwrap().unwrap();
        assert_eq!(it.copy_count, 2, "copy_count 应累加");
    }

    #[test]
    fn upsert_detects_type() {
        let c = open_in_memory().unwrap();
        let (id, _) = add(&c, r#"{"a":1}"#);
        assert_eq!(get(&c, id).unwrap().unwrap().content_type, "json");
    }

    #[test]
    fn list_orders_purely_by_last_copied_time() {
        let c = open_in_memory().unwrap();
        let (a, _) = add(&c, "older");
        let (b, _) = add(&c, "newer");
        toggle_favorite(&c, a).unwrap();
        let got = list(&c, &Query::default()).unwrap();
        assert_eq!(got.len(), 2);
        // 收藏不再置顶：较新的普通项排在前，收藏项按时间待在原位。
        // 找收藏靠「仅收藏」筛选，不靠排序特权
        assert_eq!(got[0].id, b, "较新的项应在前，即使另一条是收藏");
        assert_eq!(got[1].id, a);
    }

    /// trigram 的全部意义：搜子串能命中
    #[test]
    fn fts_matches_substring() {
        let c = open_in_memory().unwrap();
        add(&c, "select * from users");
        add(&c, "sk_live_abc123xyz");
        for q in ["ser", "use", "live_abc", "abc123"] {
            let got = list(
                &c,
                &Query {
                    text: Some(q.into()),
                    ..Default::default()
                },
            )
            .unwrap();
            assert!(!got.is_empty(), "子串 {q:?} 应命中");
        }
    }

    /// 用户输入里的 FTS5 语法字符不能让 SQL 报错
    #[test]
    fn fts_survives_special_characters() {
        let c = open_in_memory().unwrap();
        add(&c, r#"say "hi" now"#);
        for q in [
            r#""hi""#, "*", "a*b", "NEAR(", "a AND b", "^x", "col:val", r#"\\"#,
        ] {
            let r = list(
                &c,
                &Query {
                    text: Some(q.into()),
                    ..Default::default()
                },
            );
            assert!(r.is_ok(), "输入 {q:?} 不应导致 SQL 报错");
        }
    }

    #[test]
    fn fts_escapes_double_quote() {
        let c = open_in_memory().unwrap();
        add(&c, r#"say "hi" now"#);
        let got = list(
            &c,
            &Query {
                text: Some(r#""hi""#.into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(got.len(), 1, "带引号的短语应能搜到");
    }

    #[test]
    fn empty_query_returns_everything() {
        let c = open_in_memory().unwrap();
        for i in 0..5 {
            add(&c, &format!("item {i}"));
        }
        assert_eq!(list(&c, &Query::default()).unwrap().len(), 5);
        // 纯空白应视作无查询，不是搜空白串
        assert_eq!(
            list(
                &c,
                &Query {
                    text: Some("   ".into()),
                    ..Default::default()
                }
            )
            .unwrap()
            .len(),
            5
        );
    }

    #[test]
    fn filters_by_type_and_favorite() {
        let c = open_in_memory().unwrap();
        let (j, _) = add(&c, r#"{"a":1}"#);
        add(&c, "select 1 from t");
        let jsons = list(
            &c,
            &Query {
                types: Some(vec!["json".into()]),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(jsons.len(), 1);
        assert_eq!(jsons[0].id, j);

        toggle_favorite(&c, j).unwrap();
        let favs = list(
            &c,
            &Query {
                favorite_only: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(favs.len(), 1);
        assert_eq!(favs[0].id, j);
    }

    #[test]
    fn limit_and_offset() {
        let c = open_in_memory().unwrap();
        for i in 0..10 {
            add(&c, &format!("row {i}"));
        }
        let page1 = list(
            &c,
            &Query {
                limit: Some(3),
                ..Default::default()
            },
        )
        .unwrap();
        let page2 = list(
            &c,
            &Query {
                limit: Some(3),
                offset: Some(3),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page1.len(), 3);
        assert_eq!(page2.len(), 3);
        let ids1: Vec<_> = page1.iter().map(|i| i.id).collect();
        let ids2: Vec<_> = page2.iter().map(|i| i.id).collect();
        assert!(ids1.iter().all(|i| !ids2.contains(i)), "两页不应重叠");
    }

    #[test]
    fn remove_and_clear() {
        let c = open_in_memory().unwrap();
        let (a, _) = add(&c, "one");
        let (b, _) = add(&c, "two");
        remove(&c, &[a]).unwrap();
        assert_eq!(count(&c).unwrap(), 1);
        assert!(get(&c, a).unwrap().is_none());
        clear(&c).unwrap();
        assert_eq!(count(&c).unwrap(), 0);
        assert!(get(&c, b).unwrap().is_none(), "clear 后不应有残留");
    }

    #[test]
    fn update_changes_content_and_type() {
        let c = open_in_memory().unwrap();
        let (id, _) = add(&c, "hello");
        let it = update_content(&c, id, r#"{"a":1}"#, false, None)
            .unwrap()
            .expect("存在的 id 应返回更新后的条目");
        assert_eq!(it.content, r#"{"a":1}"#);
        assert_eq!(it.content_type, "json", "类型要按新内容重跑识别");
        assert_eq!(it.byte_size, 7);
    }

    /// FTS 由 UPDATE 触发器同步（db.rs 有单独的触发器测试），
    /// 这里守的是「编辑后的内容要能搜到、旧的搜不到」这条用户可见行为
    #[test]
    fn updated_content_is_searchable_old_is_not() {
        let c = open_in_memory().unwrap();
        let (id, _) = add(&c, "hello world");
        update_content(&c, id, "goodbye world", false, None).unwrap();

        let q = |t: &str| Query {
            text: Some(t.into()),
            ..Default::default()
        };
        let got = list(&c, &q("goodbye")).unwrap();
        assert_eq!(got.len(), 1, "新内容应能搜到");
        assert_eq!(got[0].id, id);
        assert!(
            list(&c, &q("hello")).unwrap().is_empty(),
            "旧内容不应再命中"
        );
    }

    #[test]
    fn update_keeps_count_and_times() {
        let c = open_in_memory().unwrap();
        let (id, _) = add(&c, "hello");
        let before = get(&c, id).unwrap().unwrap();
        update_content(&c, id, "changed", false, None).unwrap();
        let after = get(&c, id).unwrap().unwrap();
        assert_eq!(after.copy_count, before.copy_count, "编辑不是复制");
        assert_eq!(after.created_at, before.created_at);
        assert_eq!(after.last_copied_at, before.last_copied_at);
    }

    #[test]
    fn update_rejects_duplicate_of_another_row() {
        let c = open_in_memory().unwrap();
        let (a, _) = add(&c, "aaa");
        let (b, _) = add(&c, "bbb");
        let e = update_content(&c, a, "bbb", false, None).unwrap_err();
        assert!(e.to_string().contains("已有相同内容"), "实际：{e}");
        // 改成自己的内容不算冲突（空操作但要成功）
        assert!(update_content(&c, b, "bbb", false, None).unwrap().is_some());
    }

    #[test]
    fn update_missing_id_returns_none() {
        let c = open_in_memory().unwrap();
        assert!(update_content(&c, 404, "x", false, None).unwrap().is_none());
    }

    /// 敏感标记跟着**新内容**走：改出来一段密钥要被打标并带上 TTL，
    /// 反过来改掉密钥后要回到永久历史 —— 两个方向都由调用方传入，
    /// 仓库层只负责落库
    #[test]
    fn update_persists_sensitive_and_expiry() {
        let c = open_in_memory().unwrap();
        let (id, _) = add(&c, "plain");
        let ttl = now_ms() + 60_000;
        let it = update_content(&c, id, "sk_live_abcdefghijklmnopqr", true, Some(ttl))
            .unwrap()
            .unwrap();
        assert!(it.sensitive);
        assert_eq!(it.expires_at, Some(ttl));

        let it = update_content(&c, id, "plain again", false, None)
            .unwrap()
            .unwrap();
        assert!(!it.sensitive);
        assert_eq!(it.expires_at, None, "改回普通内容要清掉 TTL");
    }

    #[test]
    fn prune_keeps_favorites() {
        let c = open_in_memory().unwrap();
        let (keep, _) = add(&c, "keep me");
        let (drop, _) = add(&c, "drop me");
        toggle_favorite(&c, keep).unwrap();
        // 把时间戳显式推到过去。不这么做的话这条测试得靠墙钟
        // 往下走一毫秒才通过 —— 实测 8 次里挂 2 次。
        // 「prune 会删掉该删的」不该取决于跑得多快
        conn_set_created(&c, drop, now_ms() - 10_000);
        // retention=0 表示"立即过期"，会把非收藏的全删掉
        prune(&c, 0, Some(0)).unwrap();
        assert_eq!(count(&c).unwrap(), 1);
        assert!(get(&c, keep).unwrap().is_some(), "收藏项不该被清理");
    }

    #[test]
    fn prune_respects_max_items() {
        let c = open_in_memory().unwrap();
        for i in 0..10 {
            add(&c, &format!("m{i}"));
        }
        prune(&c, 3, None).unwrap();
        assert_eq!(count(&c).unwrap(), 3, "应只留最近 3 条");
    }

    #[test]
    fn expired_returns_due_ids() {
        let c = open_in_memory().unwrap();
        let (id, _) = add(&c, "temp");
        assert!(expired(&c, now_ms()).unwrap().is_empty());
        conn_set_expiry(&c, id, now_ms() - 1000);
        assert_eq!(expired(&c, now_ms()).unwrap(), vec![id]);
    }

    /// 清理只带走到期项：返回的是被删内容，没到期的留在库里。
    /// 这是「60 秒后自动消失」验收的核心逻辑
    #[test]
    fn purge_expired_removes_only_due_items() {
        let c = open_in_memory().unwrap();
        let (gone, _) = add_sensitive(&c, "sk_live_abcdefghijklmnopqr", -1_000);
        let (stay, _) = add_sensitive(&c, "AKIAIOSFODNN7EXAMPLE", 60_000);
        add(&c, "plain");

        let removed = purge_expired(&c, now_ms()).unwrap();
        assert_eq!(removed, vec!["sk_live_abcdefghijklmnopqr".to_string()]);
        assert!(get(&c, gone).unwrap().is_none(), "到期的应已删除");
        assert!(get(&c, stay).unwrap().is_some(), "没到期的不该被带走");
        assert_eq!(count(&c).unwrap(), 2);

        // 再清一遍是空操作，不报错也不重复返回
        assert!(purge_expired(&c, now_ms()).unwrap().is_empty());
    }

    /// docs/03：敏感项默认不参与普通结果，展开「含敏感」分组才可见。
    /// 不分搜索词与否，两条路径都得排除
    #[test]
    fn sensitive_hidden_unless_group_expanded() {
        let c = open_in_memory().unwrap();
        let (sid, _) = add_sensitive(&c, "sk_live_abcdefghijklmnopqr", 60_000);
        add(&c, "harmless note");

        let plain = list(&c, &Query::default()).unwrap();
        assert_eq!(plain.len(), 1, "默认列表不该出现敏感项");
        assert!(plain.iter().all(|i| i.id != sid));

        let searched = list(
            &c,
            &Query {
                text: Some("sk_live".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(searched.is_empty(), "默认搜索也不该搜到敏感项");

        let expanded = list(
            &c,
            &Query {
                include_sensitive: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(expanded.len(), 2, "展开分组后敏感项可见");
        assert!(expanded.iter().any(|i| i.id == sid));
        assert!(
            expanded.iter().any(|i| i.sensitive),
            "敏感标记要带出去给 UI 打锁"
        );
    }

    /// 敏感项被再次复制说明用户还在用它，到期时间必须顺延 ——
    /// 否则粘到一半条目就从历史里消失了
    #[test]
    fn recopying_sensitive_item_extends_expiry() {
        let c = open_in_memory().unwrap();
        let content = "ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let (id, _) = add_sensitive(&c, content, -30_000); // 已到期但还没被清
                                                           // 再复制一次，带新的 60 秒
        let (id2, created) = add_sensitive(&c, content, 60_000);
        assert!(!created, "同内容应命中去重");
        assert_eq!(id, id2);

        let it = get(&c, id).unwrap().unwrap();
        assert_eq!(it.copy_count, 2);
        assert!(
            it.expires_at.unwrap() > now_ms(),
            "到期时间应已被顺延到未来，实际 {:?}",
            it.expires_at
        );
    }

    /// prune 的 SQL 显式排除 expires_at IS NOT NULL 的行（见 prune 实现）——
    /// 敏感项按「到期才删」走自己的生命周期，
    /// 保留期清理提前删掉它等于绕过设计
    #[test]
    fn prune_leaves_pending_sensitive_alone() {
        let c = open_in_memory().unwrap();
        let (sid, _) = add_sensitive(&c, "xoxb-1234567890123456", 60_000);
        add(&c, "ordinary");
        // retention=0：非收藏、非敏感的全删
        prune(&c, 0, Some(0)).unwrap();
        assert!(
            get(&c, sid).unwrap().is_some(),
            "还没到期的敏感项不该被保留期清理删掉"
        );
    }

    fn conn_set_expiry(c: &Connection, id: i64, at: i64) {
        c.execute(
            "UPDATE clipboard_item SET expires_at = ?2 WHERE id = ?1",
            params![id, at],
        )
        .unwrap();
    }

    fn conn_set_created(c: &Connection, id: i64, at: i64) {
        c.execute(
            "UPDATE clipboard_item SET created_at = ?2 WHERE id = ?1",
            params![id, at],
        )
        .unwrap();
    }

    #[test]
    fn preview_truncates_and_flattens() {
        assert_eq!(preview("a\n  b\tc", 100), "a b c");
        let long = "x".repeat(300);
        let p = preview(&long, 200);
        assert_eq!(p.chars().count(), 201, "200 字符 + 省略号");
    }

    /// trigram 搜不到两字词，这是它的固有下限
    #[test]
    fn short_query_falls_back_to_like() {
        let c = open_in_memory().unwrap();
        let (id, _) = add(&c, "中文内容测试");
        // 2 字符 < trigram 的 3 字符下限，必须走 LIKE 兜底
        let got = list(
            &c,
            &Query {
                text: Some("中文".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(got.len(), 1, "两字中文词应能搜到");
        assert_eq!(got[0].id, id);

        // 单字也要能搜
        let got = list(
            &c,
            &Query {
                text: Some("中".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(got.len(), 1, "单字也应能搜");
    }

    /// 短的英文词同样受影响，回退逻辑要一致
    #[test]
    fn short_ascii_query_falls_back_to_like() {
        let c = open_in_memory().unwrap();
        let (id, _) = add(&c, "ab cd ef");
        let got = list(
            &c,
            &Query {
                text: Some("ab".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, id);
    }

    /// LIKE 的通配符必须转义，否则搜 "50%" 会匹配全表
    #[test]
    fn like_escapes_wildcards() {
        let c = open_in_memory().unwrap();
        add(&c, "progress 50% done");
        add(&c, "progress 51% done");
        let got = list(
            &c,
            &Query {
                text: Some("50%".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(got.len(), 1, "50% 应只匹配字面量 50%");
        assert!(got[0].content.contains("50%"));

        // 下划线同理
        let got = list(
            &c,
            &Query {
                text: Some("_".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(got.is_empty(), "单下划线不该匹配任意字符");
    }

    #[test]
    fn unicode_content_is_measured_in_chars() {
        let c = open_in_memory().unwrap();
        let (id, _) = add(&c, "中文内容测试");
        let got = list(
            &c,
            &Query {
                text: Some("中文内".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(got.len(), 1, "3 字中文词走 trigram 也应命中");
        assert_eq!(got[0].id, id);
    }

    /// 验收线：1000 条下搜索 < 50ms
    #[test]
    fn search_1000_rows_under_50ms() {
        let c = open_in_memory().unwrap();
        for i in 0..1000 {
            add(
                &c,
                &format!("SELECT id, name, email FROM users_{i} WHERE active = 1"),
            );
        }
        assert_eq!(count(&c).unwrap(), 1000);

        let t = std::time::Instant::now();
        let got = list(
            &c,
            &Query {
                text: Some("users_42".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        assert!(!got.is_empty(), "应命中");
        assert!(ms < 50.0, "搜索耗时 {ms:.1}ms，超出 50ms 验收线");
        println!("1000 条搜索耗时 {ms:.2}ms");
    }
}
