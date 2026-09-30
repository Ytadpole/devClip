//! JSON 变换
//!
//! ## 键序已经是排好的，所以没有 sort_keys 这个动作
//!
//! docs/04 列了「Sort Keys（按 key 字母序排序，便于 diff）」。
//! 实现时发现**它是个空操作**：`serde_json` 的 `Map` 默认是
//! `BTreeMap`，除非显式开 `preserve_order` feature，否则序列化出来
//! 天然按字母序。
//!
//! 检查过依赖树（`cargo tree -i serde_json -e features`），
//! `preserve_order` 确实没被任何人打开。所以 `format` 的输出已经
//! 排序过了，再挂一个同名的动作只会让用户以为「不点就没排」。
//! 这个动作删掉了，`format` 的 hint 里说明键已排序。

use serde::Serialize;
use serde_json::ser::PrettyFormatter;
use serde_json::{Serializer, Value};

/// 把 JSON 美化成 2 空格缩进
pub fn format(input: &str) -> Result<String, String> {
    pretty(input, 2)
}

/// 4 空格缩进。`to_string_pretty` 写死了 2 空格，只能自己配 formatter
pub fn format4(input: &str) -> Result<String, String> {
    pretty(input, 4)
}

fn pretty(input: &str, indent: usize) -> Result<String, String> {
    let v = parse(input)?;
    // 空数组与空对象要打成 [] 和 {}，PrettyFormatter 默认也是这么做的，
    // 但 Value 的顶层如果本身是空的，serde 会给 "null" 之外的表示，
    // 交给 formatter 处理即可
    let mut buf = Vec::with_capacity(input.len() * 2);
    let spaces = vec![b' '; indent];
    let mut ser = Serializer::with_formatter(&mut buf, PrettyFormatter::with_indent(&spaces));
    v.serialize(&mut ser)
        .map_err(|e| format!("序列化失败：{e}"))?;
    // from_utf8 而不是 from_utf8_lossy：内容是我们自己刚序列化的，
    // 出现非法 UTF-8 说明有 bug，静默替换成 U+FFFD 只会把 bug 藏起来
    String::from_utf8(buf).map_err(|e| format!("序列化结果不是合法 UTF-8：{e}"))
}

/// 压成一行
pub fn minify(input: &str) -> Result<String, String> {
    let v = parse(input)?;
    serde_json::to_string(&v).map_err(|e| format!("序列化失败：{e}"))
}

fn parse(input: &str) -> Result<Value, String> {
    // 错误信息要能指导下一步（docs/04 的错误处理约定），
    // serde 的原文带行列号，对着手上的内容确实有用，所以只加个前缀
    serde_json::from_str(input).map_err(|e| {
        format!(
            "不是合法的 JSON，无法处理（{}）",
            first_line(&e.to_string())
        )
    })
}

/// serde 的错误可能跨行（unexpected end of JSON input 之类），
/// 状态栏只有一行，截断到第一个换行
fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or(s).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"b":2,"a":{"z":1,"y":[3,2]}}"#;

    #[test]
    fn format_uses_two_spaces_and_sorts_keys() {
        // 键序是 serde_json::Map（BTreeMap）给的，不是我们排的
        assert_eq!(
            format(SAMPLE).unwrap(),
            "{\n  \"a\": {\n    \"y\": [\n      3,\n      2\n    ],\n    \"z\": 1\n  },\n  \"b\": 2\n}"
        );
    }

    #[test]
    fn format4_uses_four_spaces() {
        let out = format4(SAMPLE).unwrap();
        assert!(out.starts_with("{\n    \"a\": {\n        \"y\""), "{out}");
    }

    #[test]
    fn minify_is_one_line_and_parses_back() {
        let out = minify(SAMPLE).unwrap();
        assert!(!out.contains('\n'), "{out}");
        let back: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(back["b"], 2);
        assert_eq!(back["a"]["z"], 1);
    }

    #[test]
    fn minify_preserves_array_order() {
        // 排序只作用于对象的键。数组顺序是有语义的，
        // 排了就是把数据改了 —— 这条要是坏了，用户会拿到错的结果
        let out = minify(r#"{"a":[3,1,2]}"#).unwrap();
        assert_eq!(out, r#"{"a":[3,1,2]}"#);
    }

    #[test]
    fn rejects_invalid_json_with_readable_text() {
        let e = format("{oops}").unwrap_err();
        assert!(e.starts_with("不是合法的 JSON"), "{e}");
        assert!(!e.contains('\n'), "错误信息不能跨行：{e}");
    }

    #[test]
    fn rejects_trailing_comma() {
        // 用户从网页上复制的 JSON 经常带尾逗号。这类「差一点」的输入
        // 报错要说清是哪一处，不然用户以为是工具坏了
        let e = format(r#"{"a":1,}"#).unwrap_err();
        assert!(e.contains("不是合法的 JSON"), "{e}");
    }

    #[test]
    fn empty_containers_survive() {
        // 顶层就是 [] 或 {} 的输入不少见（从接口复制了个空数组）
        assert_eq!(format("[]").unwrap(), "[]");
        assert_eq!(format("{}").unwrap(), "{}");
        assert_eq!(minify("[]").unwrap(), "[]");
    }

    #[test]
    fn bare_number_is_valid_json() {
        // 与 detect.rs 保持一致：裸数字是合法 JSON。
        // 这里不一致的话会出现「识别成 json 却不能格式化」
        assert_eq!(minify("6379").unwrap(), "6379");
    }

    #[test]
    fn format_is_idempotent() {
        // 格式化两次应该得到同样的结果。SQL 那边有对应的性质，
        // JSON 这边靠 BTreeMap 保证 —— 但值得钉住，
        // 因为一旦有人开了 preserve_order 就会静默失效
        let once = format(SAMPLE).unwrap();
        assert_eq!(format(&once).unwrap(), once);
    }
}
