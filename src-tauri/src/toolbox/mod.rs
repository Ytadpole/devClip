//! 工具箱 —— 阶段 6
//!
//! 与普通剪贴板管理器的分界线：不是「存下来」，而是「理解并改写它」。
//! 规格见 docs/04。
//!
//! ## 为什么是注册表
//!
//! docs/04 明确要求「加新类型不用动分发逻辑」。这里照做，但有一处
//! 与原始设计的偏离：`run` 的类型是 `fn(&str) -> Result<String, String>`，
//! 不接受 `&AppHandle`。
//!
//! 原因是**可测**。六类动作里有五类是纯字符串变换，一个不需要
//! 起 Tauri 应用的单元测试比真机点一遍便宜太多 —— 而 SQL 格式化
//! 这种逻辑恰恰是最需要测试兜底的。
//!
//! 需要应用句柄的动作（打开浏览器）不进注册表，走单独的 command。

mod b64;
mod json;
mod jwt;
mod sql;
mod url_tool;
mod uuid_tool;

use serde::Serialize;

use crate::detect::ContentType;

/// 动作在表里的样子。序列化后就是 `availableActions()` 的返回元素，
/// 前端直接拿去渲染，不含任何类型判断逻辑
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolboxAction {
    pub id: String,
    pub label: String,
    /// 前端用来解释结果的副标题。比如 JWT 那几条要标明
    /// 「base64 不是加密」，不能只给一个 "Decode Header"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// `transform`（默认）走 `run_toolbox_action`；`open` 走
    /// `open_external`
    ///
    /// 分开是因为两者要的东西根本不同：变换是纯函数、结果进剪贴板，
    /// 而「在浏览器打开」要唤起系统默认程序 —— 它进不了注册表
    /// （见 `Entry` 上面对 `run` 的要求），所以由后端声明、前端路由
    #[serde(default, skip_serializing_if = "is_default_kind")]
    pub kind: &'static str,
}

fn is_default_kind(k: &str) -> bool {
    k == "transform"
}

/// `url.open` 的 id。前端按它路由，而 `find()` 取不到它 ——
/// 需要 `AppHandle` 的动作不在 ENTRIES 里
pub const OPEN_IN_BROWSER_ID: &str = "url.open";

/// 注册表里的一项。
///
/// `run` 只吃输入、只产出结果，不碰系统状态 —— 这是它能被
/// 单元测试直接覆盖的原因
pub struct Entry {
    pub id: &'static str,
    pub label: &'static str,
    pub hint: Option<&'static str>,
    pub applies_to: &'static [ContentType],
    pub run: fn(&str) -> Result<String, String>,
}

/// 按出现顺序排列。同类型的动作在工具条上就是这个顺序，
/// 「破坏性」的动作（minify、strip_query）排后面 —— 用户是
/// 顺着点的，最常见的该在最左边
pub static ENTRIES: &[Entry] = &[
    Entry {
        id: "json.format",
        label: "美化",
        hint: Some("2 空格缩进"),
        applies_to: &[ContentType::Json],
        run: json::format,
    },
    Entry {
        id: "json.format4",
        label: "美化 (4 空格)",
        hint: Some("4 空格缩进"),
        applies_to: &[ContentType::Json],
        run: json::format4,
    },
    Entry {
        id: "json.minify",
        label: "压缩",
        hint: Some("压成一行"),
        applies_to: &[ContentType::Json],
        run: json::minify,
    },
    Entry {
        id: "jwt.decode_header",
        label: "解 Header",
        hint: Some("base64 不是加密"),
        applies_to: &[ContentType::Jwt],
        run: jwt::decode_header,
    },
    Entry {
        id: "jwt.decode_payload",
        label: "解 Payload",
        hint: Some("base64 不是加密"),
        applies_to: &[ContentType::Jwt],
        run: jwt::decode_payload,
    },
    Entry {
        id: "jwt.verify_exp",
        label: "检查过期",
        hint: Some("只读 exp，不验签"),
        applies_to: &[ContentType::Jwt],
        run: jwt::verify_exp,
    },
    Entry {
        id: "b64.decode",
        label: "解码",
        hint: None,
        applies_to: &[ContentType::Base64],
        run: b64::decode,
    },
    Entry {
        id: "b64.decode_urlsafe",
        label: "解码 (url-safe)",
        hint: Some("字母表 -_ 而非 +/"),
        applies_to: &[ContentType::Base64],
        run: b64::decode_urlsafe,
    },
    Entry {
        id: "b64.encode",
        // 挂在 text / code 上是有意的：把一段命令或正则编成 base64
        // 是常见需求，而这类内容的类型必然是 text 或 code。
        // 代价是选中任何一句话都会出现工具条，对开发者工具可以接受
        label: "编码",
        hint: Some("任意文本 → Base64"),
        applies_to: &[ContentType::Base64, ContentType::Text, ContentType::Code],
        run: b64::encode,
    },
    Entry {
        id: "b64.encode_urlsafe",
        label: "编码 (url-safe)",
        hint: Some("字母表 -_ 而非 +/"),
        applies_to: &[ContentType::Base64, ContentType::Text, ContentType::Code],
        run: b64::to_urlsafe,
    },
    Entry {
        id: "sql.format",
        label: "格式化",
        hint: Some("关键字大写 + 换行"),
        applies_to: &[ContentType::Sql],
        run: sql::format,
    },
    Entry {
        id: "sql.upper",
        label: "关键字大写",
        hint: None,
        applies_to: &[ContentType::Sql],
        run: sql::upper,
    },
    Entry {
        id: "sql.lower",
        label: "关键字小写",
        hint: None,
        applies_to: &[ContentType::Sql],
        run: sql::lower,
    },
    Entry {
        id: "sql.tables",
        label: "提取表名",
        hint: Some("只解析，不连库"),
        applies_to: &[ContentType::Sql],
        run: sql::tables,
    },
    Entry {
        id: "url.strip_query",
        label: "去掉 query",
        applies_to: &[ContentType::Url],
        hint: None,
        run: url_tool::strip_query,
    },
    Entry {
        id: "url.domain",
        label: "提取域名",
        applies_to: &[ContentType::Url],
        hint: None,
        run: url_tool::domain,
    },
    Entry {
        id: "uuid.upper",
        label: "转大写",
        applies_to: &[ContentType::Uuid],
        hint: None,
        run: uuid_tool::upper,
    },
    Entry {
        id: "uuid.lower",
        label: "转小写",
        applies_to: &[ContentType::Uuid],
        hint: None,
        run: uuid_tool::lower,
    },
    Entry {
        id: "uuid.no_dashes",
        label: "去横线",
        hint: Some("MySQL bin(16) 用这个"),
        applies_to: &[ContentType::Uuid],
        run: uuid_tool::no_dashes,
    },
];

/// 该类型可用的动作
pub fn for_type(t: ContentType) -> Vec<ToolboxAction> {
    let mut out: Vec<ToolboxAction> = ENTRIES
        .iter()
        .filter(|e| e.applies_to.contains(&t))
        .map(|e| ToolboxAction {
            id: e.id.to_string(),
            label: e.label.to_string(),
            hint: e.hint.map(str::to_string),
            kind: "transform",
        })
        .collect();
    // 需要 AppHandle 的动作在最后补进去，顺序刻意放在变换之后 ——
    // 它不产生剪贴板内容，和前面几条不是一回事
    if t == ContentType::Url {
        out.push(ToolboxAction {
            id: OPEN_IN_BROWSER_ID.to_string(),
            label: "在浏览器打开".to_string(),
            hint: Some("交给系统默认的浏览器，不经过剪贴板".to_string()),
            kind: "open",
        });
    }
    out
}

/// 按 id 取动作。取不到就是前端传了不该传的 id
pub fn find(id: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|e| e.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_type_returns_actions_in_registry_order() {
        let ids: Vec<_> = for_type(ContentType::Json)
            .into_iter()
            .map(|a| a.id)
            .collect();
        assert_eq!(ids, ["json.format", "json.format4", "json.minify"]);
    }

    #[test]
    fn text_gets_the_two_encoding_actions() {
        // text 挂的是 base64 编码类动作。顺序有讲究：标准在前、
        // url-safe 在后 —— 前者用得多，后者是「发现 +/ 出问题了」才点的
        let ids: Vec<_> = for_type(ContentType::Text)
            .into_iter()
            .map(|a| a.id)
            .collect();
        assert_eq!(ids, ["b64.encode", "b64.encode_urlsafe"]);
    }

    #[test]
    fn image_and_code_have_no_actions() {
        assert!(for_type(ContentType::Image).is_empty());
        // markdown / exception 阶段 6 没做动作
        assert!(for_type(ContentType::Markdown).is_empty());
    }

    #[test]
    fn every_id_is_unique() {
        let mut ids: Vec<_> = ENTRIES.iter().map(|e| e.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "动作 id 重复");
    }

    #[test]
    fn ids_are_namespaced_by_type() {
        // id 的前缀必须与 applies_to 对得上，否则前端拿 json 的动作
        // 去跑一条 sql 会走到不相干的实现上，而报错还很难懂
        for e in ENTRIES {
            let ns = e.id.split('.').next().unwrap();
            let want = match ns {
                "json" => ContentType::Json,
                "jwt" => ContentType::Jwt,
                "b64" => ContentType::Base64,
                "sql" => ContentType::Sql,
                "url" => ContentType::Url,
                "uuid" => ContentType::Uuid,
                other => panic!("未知命名空间 {other}"),
            };
            assert!(
                e.applies_to.contains(&want),
                "{} 的 id 前缀与 applies_to 不符",
                e.id
            );
        }
    }

    /// 动作能不能用，实际是拿**数据库里的字符串**判断的（见 lib.rs
    /// 的 run_toolbox_action），所以得钉住「字符串 → 枚举」这条路，
    /// 而不是只测枚举本身。as_str/from_str 少写一个分支的话，
    /// 那一条类型的工具条会静默消失 —— 而它只是少几个按钮，不报错
    #[test]
    fn type_strings_round_trip() {
        for want in [
            ContentType::Image,
            ContentType::Json,
            ContentType::Jwt,
            ContentType::Uuid,
            ContentType::Ip,
            ContentType::Url,
            ContentType::Commit,
            ContentType::Exception,
            ContentType::Sql,
            ContentType::Base64,
            ContentType::Markdown,
            ContentType::Code,
            ContentType::Text,
        ] {
            let s = want.as_str();
            let got: ContentType = s.parse().unwrap();
            assert_eq!(got, want, "{s} 解析回来不是自己");
        }
    }

    #[test]
    fn unknown_type_falls_back_to_text() {
        // 脏数据不该把工具箱卡住。回退到 text 的后果是：一条类型
        // 认不出的记录仍能拿到 base64 编码动作，比什么都不给好用
        let got: ContentType = "nonsense".parse().unwrap();
        assert_eq!(got, ContentType::Text);
    }

    /// 「能列出来」与「能跑」必须是同一个判断。这条把两者绑在一起：
    /// for_type() 用的是 applies_to，运行时的适用性检查也用 applies_to，
    /// 一旦有人只改其中一边，UI 上就会给出点一下就报错的按钮
    ///
    /// 唯一的例外是 `kind: "open"`：它进不了注册表（要 `AppHandle`），
    /// 走的是 `open_external`。所以按 kind 分流，**例外被收窄到那一个
    /// 动作** —— 以后任何新的 open 类动作都得显式加进来，否则这条拦下
    #[test]
    fn listed_actions_are_exactly_the_runnable_ones() {
        for t in [
            ContentType::Json,
            ContentType::Jwt,
            ContentType::Base64,
            ContentType::Sql,
            ContentType::Url,
            ContentType::Uuid,
            ContentType::Text,
            ContentType::Code,
            ContentType::Image,
            ContentType::Markdown,
        ] {
            for a in for_type(t) {
                if a.kind == "open" {
                    assert_eq!(
                        (a.id.as_str(), t),
                        (OPEN_IN_BROWSER_ID, ContentType::Url),
                        "{} 是没登记过的 open 类动作",
                        a.id
                    );
                    continue;
                }
                let e = find(&a.id).expect("列出来的变换动作必须能按 id 找到");
                assert!(
                    e.applies_to.contains(&t),
                    "{} 对 {} 列出来了，但运行时会拒绝",
                    a.id,
                    t.as_str()
                );
            }
        }
    }

    /// open 类动作排在最后，且只有 url 有。它不产生剪贴板内容，
    /// 混在变换动作里会让工具条读起来像同一类操作
    #[test]
    fn open_action_is_last_and_url_only() {
        let url = for_type(ContentType::Url);
        assert_eq!(url.last().unwrap().kind, "open");
        assert_eq!(url.last().unwrap().id, OPEN_IN_BROWSER_ID);
        for t in [
            ContentType::Json,
            ContentType::Text,
            ContentType::Code,
            ContentType::Sql,
            ContentType::Jwt,
            ContentType::Base64,
            ContentType::Uuid,
        ] {
            assert!(
                for_type(t).iter().all(|a| a.kind == "transform"),
                "{} 不该有 open 类动作",
                t.as_str()
            );
        }
    }

    #[test]
    fn find_rejects_unknown_id() {
        assert!(find("json.format").is_some());
        assert!(find("json.nope").is_none());
        // 前缀匹配不算命中，否则 "sql" 会命中 "sql.format"
        assert!(find("sql").is_none());
    }

    #[test]
    fn jwt_actions_carry_the_not_encrypted_hint() {
        // docs/04 要求 UI 上明确标注 JWT 是 base64 不是加密。
        // 这个提示只在前端能看见，所以必须由后端带出去。
        // 逐条断言文案而不只是 is_some()：两条解码动作要说明 payload
        // 是明文可读的，verify_exp 要说明它不验签。三者不能互相顶替 ——
        // 「不验签」比「不是加密」更要紧，少说等于误导
        let want = [
            ("jwt.decode_header", Some("base64 不是加密")),
            ("jwt.decode_payload", Some("base64 不是加密")),
            ("jwt.verify_exp", Some("只读 exp，不验签")),
        ];
        let actions = for_type(ContentType::Jwt);
        let got: Vec<_> = actions
            .iter()
            .map(|a| (a.id.as_str(), a.hint.as_deref()))
            .collect();
        assert_eq!(got, want);
    }
}
