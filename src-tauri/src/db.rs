//! SQLite 连接与 schema —— 阶段 4
//!
//! 对应 docs/03-数据库设计.md。要点：
//! - WAL 模式，历史写入与前端查询不互相阻塞
//! - FTS5 用 **trigram** 分词器（不是 unicode61）：搜代码要的是子串匹配，
//!   `unicode61` 会把 `sk_live_abc` 切成三段，搜 `live_abc` 完全匹配不上
//! - trigram 需要 SQLite >= 3.34，靠 rusqlite 的 bundled feature 保证

use rusqlite::Connection;
use std::fmt;
use std::path::Path;

/// 库层错误。IO 与 SQL 分开，因为给用户看的提示不一样：
/// IO 失败多半是权限或磁盘满，SQL 失败多半是查询写错了。
/// Conflict 是「操作本身不被允许」而不是坏掉了 —— 提示要指导
/// 下一步（比如「历史里已有相同内容」），套进「数据库错误」
/// 的措辞里用户只会以为应用坏了
#[derive(Debug)]
pub enum DbError {
    Io(String),
    Sql(rusqlite::Error),
    Conflict(String),
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbError::Io(m) => write!(f, "{m}"),
            DbError::Sql(e) => write!(f, "数据库错误: {e}"),
            DbError::Conflict(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for DbError {}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        DbError::Sql(e)
    }
}

/// schema 版本，走 PRAGMA user_version，不另建迁移表
const SCHEMA_VERSION: i64 = 1;

pub fn open(path: &Path) -> Result<Connection, DbError> {
    if let Some(dir) = path.parent() {
        // 目录不存在是正常情况（首次启动），但建不出来是真错误。
        // std::io::Error 不会自动转成 rusqlite::Error，所以显式包一层
        std::fs::create_dir_all(dir)
            .map_err(|e| DbError::Io(format!("创建数据目录失败 {}: {e}", dir.display())))?;
    }
    let conn = Connection::open(path).map_err(DbError::Sql)?;
    init(&conn).map_err(DbError::Sql)?;
    Ok(conn)
}

/// 内存库，给测试用。生产路径走 open()
#[cfg(test)]
pub fn open_in_memory() -> Result<Connection, rusqlite::Error> {
    let conn = Connection::open_in_memory()?;
    init(&conn)?;
    Ok(conn)
}

fn init(conn: &Connection) -> Result<(), rusqlite::Error> {
    // busy_timeout 要在最前面设：WAL 下写锁冲突时等待而不是立刻报错
    conn.pragma_update(None, "busy_timeout", 5000_i64)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;

    if schema_version(conn)? == SCHEMA_VERSION {
        return Ok(());
    }

    conn.execute_batch(SCHEMA)?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

fn schema_version(conn: &Connection) -> Result<i64, rusqlite::Error> {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS clipboard_item (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    content        TEXT,
    -- blake3(content)，去重依据。UNIQUE 约束是最后一道防线，
    -- 重复内容在应用层就 upsert，不该走到这里
    content_hash   TEXT    NOT NULL UNIQUE,
    content_type   TEXT    NOT NULL,
    preview        TEXT,
    -- 图片走文件系统，不进 BLOB（FTS rowid 对齐和 vacuum 都会因此变慢）
    image_path     TEXT,
    byte_size      INTEGER NOT NULL DEFAULT 0,
    -- 重复复制累加，不新增行
    copy_count     INTEGER NOT NULL DEFAULT 1,
    source_app     TEXT,
    -- 首次进入历史的时间
    created_at     INTEGER NOT NULL,
    -- 最近一次复制的时间，排序用这个
    last_copied_at INTEGER NOT NULL,
    favorite       INTEGER NOT NULL DEFAULT 0,
    sensitive      INTEGER NOT NULL DEFAULT 0,
    -- 到期自动清除（毫秒时间戳），NULL 表示不过期
    expires_at     INTEGER
);

CREATE INDEX IF NOT EXISTS idx_item_recent   ON clipboard_item(last_copied_at DESC);
CREATE INDEX IF NOT EXISTS idx_item_favorite ON clipboard_item(favorite, last_copied_at DESC);
CREATE INDEX IF NOT EXISTS idx_item_type     ON clipboard_item(content_type, last_copied_at DESC);
-- 部分索引：绝大多数行 expires_at 是 NULL，不该进索引
CREATE INDEX IF NOT EXISTS idx_item_expiry   ON clipboard_item(expires_at)
    WHERE expires_at IS NOT NULL;

-- 外部内容表：只索引 content 列，省空间
CREATE VIRTUAL TABLE IF NOT EXISTS item_fts USING fts5(
    content,
    content = 'clipboard_item',
    content_rowid = 'id',
    tokenize = 'trigram'
);

-- 触发器保持 FTS 与主表同步。外部内容表不能直接 UPDATE，
-- 必须先 'delete' 再插入旧值
CREATE TRIGGER IF NOT EXISTS clipboard_item_ai AFTER INSERT ON clipboard_item BEGIN
    INSERT INTO item_fts(rowid, content) VALUES (new.id, new.content);
END;

CREATE TRIGGER IF NOT EXISTS clipboard_item_ad AFTER DELETE ON clipboard_item BEGIN
    INSERT INTO item_fts(item_fts, rowid, content) VALUES ('delete', old.id, old.content);
END;

CREATE TRIGGER IF NOT EXISTS clipboard_item_au AFTER UPDATE ON clipboard_item BEGIN
    INSERT INTO item_fts(item_fts, rowid, content) VALUES ('delete', old.id, old.content);
    INSERT INTO item_fts(rowid, content) VALUES (new.id, new.content);
END;
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
    }

    fn insert(conn: &Connection, id: i64, content: &str) {
        conn.execute(
            "INSERT INTO clipboard_item (id, content, content_hash, content_type, created_at, last_copied_at)
             VALUES (?1, ?2, ?3, 'text', ?4, ?4)",
            rusqlite::params![id, content, format!("h{id}"), now()],
        )
        .unwrap();
    }

    #[test]
    fn opens_and_sets_pragmas() {
        let conn = open_in_memory().unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_ne!(mode, "delete", "WAL 应已开启");
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn fts5_trigram_is_available() {
        // trigram 是 3.34 引入的，靠 bundled feature 保证
        let conn = open_in_memory().unwrap();
        let v: String = conn
            .query_row("SELECT sqlite_version()", [], |r| r.get(0))
            .unwrap();
        assert!(
            version_ge(&v, "3.34.0"),
            "SQLite {v} 不支持 trigram，需要 >= 3.34"
        );
    }

    fn version_ge(a: &str, b: &str) -> bool {
        let pa: Vec<u32> = a.split('.').map(|s| s.parse().unwrap_or(0)).collect();
        let pb: Vec<u32> = b.split('.').map(|s| s.parse().unwrap_or(0)).collect();
        for i in 0..3 {
            let x = pa.get(i).copied().unwrap_or(0);
            let y = pb.get(i).copied().unwrap_or(0);
            if x != y {
                return x > y;
            }
        }
        true
    }

    #[test]
    fn trigger_indexes_inserted_row() {
        let conn = open_in_memory().unwrap();
        insert(&conn, 1, "hello world");
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM item_fts WHERE item_fts MATCH 'hello'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "INSERT 后 FTS 应可搜到");
    }

    #[test]
    fn trigger_removes_deleted_row() {
        let conn = open_in_memory().unwrap();
        insert(&conn, 1, "hello world");
        conn.execute("DELETE FROM clipboard_item WHERE id = 1", [])
            .unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM item_fts WHERE item_fts MATCH 'hello'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0, "DELETE 后 FTS 不应再命中");
    }

    #[test]
    fn trigger_syncs_updated_row() {
        let conn = open_in_memory().unwrap();
        insert(&conn, 1, "before");
        conn.execute(
            "UPDATE clipboard_item SET content = 'after' WHERE id = 1",
            [],
        )
        .unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM item_fts WHERE item_fts MATCH 'after'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "UPDATE 后应能搜到新内容");
        let n_old: i64 = conn
            .query_row(
                "SELECT count(*) FROM item_fts WHERE item_fts MATCH 'before'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n_old, 0, "旧内容应已从索引移除");
    }

    #[test]
    fn content_hash_is_unique() {
        let conn = open_in_memory().unwrap();
        insert(&conn, 1, "x");
        let r = conn.execute(
            "INSERT INTO clipboard_item (content, content_hash, content_type, created_at, last_copied_at)
             VALUES ('y', 'h1', 'text', 1, 1)",
            [],
        );
        assert!(r.is_err(), "相同 hash 应被 UNIQUE 约束拒绝");
    }
}
