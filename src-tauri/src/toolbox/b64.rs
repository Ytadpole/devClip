//! Base64 编解码
//!
//! 错误信息按 docs/04 的约定写：给出**具体数值**，而不是
//! `InvalidPadding`。用户看到「长度 13 不是 4 的倍数」能自己判断
//! 是不是被截断了，看到 `InvalidPadding` 只能干瞪眼。

use base64::Engine;

const STD: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;
const URLSAFE: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE;

/// 解标准 base64
pub fn decode(input: &str) -> Result<String, String> {
    decode_with(input, "base64", |s| STD.decode(s))
}

/// 解 url-safe base64（字母表 `-_` 而非 `+/`）
///
/// 两种字母表都试一遍：JWT 的 payload 就是 url-safe 的，而
/// detect.rs 会把它判成 jwt 而不是 base64 —— 但用户从别处
/// 粘来的 url-safe 串会被判成 base64，那时就只有这个动作能救
pub fn decode_urlsafe(input: &str) -> Result<String, String> {
    let s = input.trim();
    let url_err = decode_with(s, "base64url", |x| URLSAFE.decode(x));
    match url_err {
        Ok(v) => Ok(v),
        Err(e) => decode_with(s, "base64url", |x| STD.decode(x)).map_err(|_| e),
    }
}

fn decode_with(
    input: &str,
    name: &str,
    f: impl Fn(&str) -> Result<Vec<u8>, base64::DecodeError>,
) -> Result<String, String> {
    if input.trim().is_empty() {
        return Err(format!("内容为空，没有可解码的 {name}"));
    }
    // 空白必须**剥掉再解**。从终端或 PDF 里复制的 base64 常带换行，
    // 而 base64 的字母表里没有换行 —— 只在校验长度时忽略它、
    // 解码时又传原文，会得到一句莫名其妙的 InvalidLastSymbol
    let s: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    // 长度检查放在解码之前：DecodeError 里没有「原串有多长」，
    // 而这是用户最需要的那个数
    if !s.len().is_multiple_of(4) {
        return Err(format!(
            "{name} 长度不是 4 的倍数（去掉空白后 {}）",
            s.len()
        ));
    }
    let bytes = f(&s).map_err(|e| format!("{name} 解码失败：{e}"))?;
    String::from_utf8(bytes)
        .map_err(|_| "解出来不是合法 UTF-8 —— 它编码的多半是二进制，不是文本".to_string())
}

/// 任意文本编码成标准 base64
pub fn encode(input: &str) -> Result<String, String> {
    Ok(STD.encode(input.as_bytes()))
}

/// 解完再编码回标准 base64。URL 与文件名场景常用
pub fn to_urlsafe(input: &str) -> Result<String, String> {
    Ok(URLSAFE.encode(input.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_text() {
        let enc = encode("hello 世界").unwrap();
        assert_eq!(decode(&enc).unwrap(), "hello 世界");
    }

    #[test]
    fn decodes_known_vector() {
        // RFC 4648 的标准向量，避免「解出来还是自己」的自证循环
        assert_eq!(decode("aGVsbG8=").unwrap(), "hello");
        assert_eq!(encode("hello").unwrap(), "aGVsbG8=");
    }

    #[test]
    fn reports_length_in_error() {
        // docs/04 点名要给的数值就是这个
        let e = decode("aGVsbG8").unwrap_err();
        assert!(e.contains("6"), "{e}");
        assert!(e.contains("4 的倍数"), "{e}");
    }

    #[test]
    fn ignores_surrounding_whitespace() {
        // 从终端或 PDF 里复制的 base64 常带换行
        assert_eq!(decode("aGVs\nbG8=\n").unwrap(), "hello");
    }

    #[test]
    fn rejects_empty() {
        assert!(decode("").unwrap_err().contains("内容为空"));
        assert!(decode("   ").unwrap_err().contains("内容为空"));
    }

    #[test]
    fn reports_non_utf8_as_binary_not_crash() {
        // 0xFF 0xFE 解出来不是 UTF-8。错误要说清「这是二进制」，
        // 因为用户下一步多半是想存图片而不是文本
        let enc = STD.encode([0xffu8, 0xfe, 0xfd]);
        let e = decode(&enc).unwrap_err();
        assert!(e.contains("二进制"), "{e}");
    }

    #[test]
    fn urlsafe_decodes_both_alphabets() {
        // 样本用 U+10FFFF：它的 UTF-8 编码是 F4 8F BF BF，
        // 6 位分组里正好落进 62 与 63 两个字符，而解回来仍是合法
        // UTF-8。0xFB 0xFF 那类原始字节也能造出 +/，但解不开成字符串
        const S: &str = "\u{10FFFF}";
        let url = URLSAFE.encode(S);
        assert!(url.contains('-') && url.contains('_'), "{url}");
        assert_eq!(decode_urlsafe(&url).unwrap(), S);

        // 标准字母表也要能解 —— 函数名叫 url-safe 但用户不会看文档
        let std = STD.encode(S);
        assert!(std.contains('+') && std.contains('/'), "{std}");
        assert_eq!(decode_urlsafe(&std).unwrap(), S);
    }

    #[test]
    fn urlsafe_encoding_avoids_plus_and_slash() {
        let enc = to_urlsafe("\u{10FFFF}").unwrap();
        assert!(!enc.contains('+') && !enc.contains('/'), "{enc}");
    }

    #[test]
    fn rejects_garbage_with_length_hint() {
        // 长度对但字符集非法的样本：'!' 不在任何 base64 字母表里
        let e = decode("!!!!").unwrap_err();
        assert!(e.contains("解码失败"), "{e}");
    }
}
