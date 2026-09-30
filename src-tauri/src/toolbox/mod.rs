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
//! 原因是**可测**。动作全是纯字符串变换，一个不需要起 Tauri 应用的
//! 单元测试比真机点一遍便宜太多 —— 而后面几类格式化逻辑恰恰是
//! 最需要测试兜底的。
//!
//! 需要应用句柄的动作（打开浏览器）不进注册表，走单独的 command。

mod b64;
mod json;

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
}

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
];

/// 该类型可用的动作
pub fn for_type(t: ContentType) -> Vec<ToolboxAction> {
    ENTRIES
        .iter()
        .filter(|e| e.applies_to.contains(&t))
        .map(|e| ToolboxAction {
            id: e.id.to_string(),
            label: e.label.to_string(),
            hint: e.hint.map(str::to_string),
        })
        .collect()
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

    /// id 的前缀必须与 applies_to 对得上，否则前端拿 json 的动作
    /// 去跑一条 base64 会走到不相干的实现上，而报错还很难懂
    #[test]
    fn ids_are_namespaced_by_type() {
        for e in ENTRIES {
            let want = match e.id.split('.').next().unwrap() {
                "json" => ContentType::Json,
                "b64" => ContentType::Base64,
                other => panic!("未知命名空间 {other}"),
            };
            assert!(
                e.applies_to.contains(&want),
                "{} 的 id 前缀与 applies_to 不符",
                e.id
            );
        }
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
    fn find_rejects_unknown_id() {
        assert!(find("json.format").is_some());
        assert!(find("json.nope").is_none());
        // 前缀匹配不算命中，否则 "b64" 会命中 "b64.encode"
        assert!(find("b64").is_none());
    }
}
