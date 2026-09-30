//! UUID 大小写与去横线
//!
//! 三件事里只有 `no_dashes` 有实际用途：MySQL 的 `BINARY(16)` 列、
//! 各种 API 的 path 参数要的是 32 位无横线形式。大写化的用途窄得多
//! （肉眼比对、某些日志格式），但它便宜，一并给了。
//!
//! `upper` **保留横线**、只改字母大小写。把横线一起处理的是
//! `no_dashes` —— 两个动作不能互相顶替，用户点错一个的代价是
//! 多打 4 个字符。

/// 校验并取出 32 位 hex（去掉横线后的形式）
fn normalized(input: &str) -> Result<String, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err("内容为空".to_string());
    }
    // 两种形式都收：带横线的是用户平时见到的，无横线的多半是从
    // 别的系统导出的。中间夹的横线直接去掉再判长度
    let hex: String = s.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return Err(format!(
            "不是 UUID：去掉横线后是 {} 位，应该是 32 位",
            hex.len()
        ));
    }
    if let Some(bad) = hex.chars().find(|c| !c.is_ascii_hexdigit()) {
        return Err(format!("不是 UUID：{bad} 不是十六进制字符"));
    }
    Ok(hex)
}

/// 转大写。**保留横线**
///
/// 先过 `normalized()` 校验，再按**原文**重组：只改 hex 字母的
/// 大小写，横线的位置原样留着。直接整体 to_uppercase() 也能过，
/// 但那样就分不清「改大小写」与「顺便把横线也处理了」，
/// 而这两个动作必须是分开的
pub fn upper(input: &str) -> Result<String, String> {
    normalized(input)?;
    Ok(input
        .trim()
        .chars()
        .map(|c| if c == '-' { c } else { c.to_ascii_uppercase() })
        .collect())
}

/// 转小写。**保留横线**
pub fn lower(input: &str) -> Result<String, String> {
    normalized(input)?;
    Ok(input
        .trim()
        .chars()
        .map(|c| if c == '-' { c } else { c.to_ascii_lowercase() })
        .collect())
}

/// 去掉横线，输出 32 位小写 hex
///
/// 输出统一小写：这个动作的下游是 `BINARY(16)`、URL path 这类
/// 按字节比的地方，大写版本会匹配不上。要大写的话先 `upper`
/// —— 但顺序反了就没用了，所以这里只给一种结果
pub fn no_dashes(input: &str) -> Result<String, String> {
    Ok(normalized(input)?.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CANON: &str = "550e8400-e29b-41d4-a716-446655440000";

    #[test]
    fn upper_keeps_dashes() {
        assert_eq!(
            upper(CANON).unwrap(),
            "550E8400-E29B-41D4-A716-446655440000"
        );
    }

    #[test]
    fn lower_normalizes() {
        let up = "550E8400-E29B-41D4-A716-446655440000";
        assert_eq!(lower(up).unwrap(), CANON);
        // 已经是小写的输入不该被改动
        assert_eq!(lower(CANON).unwrap(), CANON);
    }

    #[test]
    fn strips_dashes() {
        assert_eq!(
            no_dashes(CANON).unwrap(),
            "550e8400e29b41d4a716446655440000"
        );
    }

    #[test]
    fn roundtrips_through_no_dashes() {
        // 无横线 → 去横线（已是它自己）→ 补回横线应当等于原值。
        // 少一次转义或少一位 hex 的话这条会红
        let bare = no_dashes(CANON).unwrap();
        let again = no_dashes(&bare).unwrap();
        assert_eq!(bare, again);
        assert_eq!(upper(&bare).unwrap(), "550E8400E29B41D4A716446655440000");
    }

    #[test]
    fn accepts_input_without_dashes() {
        // 很多系统导出的就是无横线形式
        let bare = "550e8400e29b41d4a716446655440000";
        assert_eq!(no_dashes(bare).unwrap(), bare);
        assert_eq!(upper(bare).unwrap(), "550E8400E29B41D4A716446655440000");
    }

    #[test]
    fn rejects_wrong_length() {
        // 报错要带上实际的位数：36 位里少一位是最常见的复制事故
        let e = no_dashes("550e8400-e29b-41d4-a716").unwrap_err();
        assert!(e.contains("20 位"), "{e}");
        assert!(e.contains("32 位"), "{e}");
    }

    #[test]
    fn rejects_non_hex() {
        let e = no_dashes("550e8400-e29b-41d4-a716-44665544000g").unwrap_err();
        assert!(e.contains("十六进制"), "{e}");
        // 字母表外的字符（中文、emoji）也要落到这条错上，而不是 panic
        assert!(no_dashes("不是 uuid 不是一个 uuid 不是一个 uu").is_err());
    }

    #[test]
    fn rejects_empty() {
        assert!(no_dashes("").unwrap_err().contains("内容为空"));
        assert!(no_dashes("   ").unwrap_err().contains("内容为空"));
        // 三个动作的校验必须一致：只有一个不认非 UUID 的话，
        // 用户会以为另外两个更宽松
        assert!(upper("").unwrap_err().contains("内容为空"));
        assert!(lower("").unwrap_err().contains("内容为空"));
        assert!(lower("abc").unwrap_err().contains("32 位"));
    }
}
