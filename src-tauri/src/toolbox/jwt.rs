//! JWT 解码
//!
//! 三条动作都**不验签**，也没有验签：验签需要公钥或共享密钥，
//! 而工具箱的输入是「用户刚粘进来的一个串」，没有任何密钥可用。
//! `verify_exp` 的 hint 因此写的是「只读 exp，不验签」——
//! 一个说成「检查过期」就足以让人以为签名验过了。
//!
//! **payload 是 base64 编码，不是加密。** 任何人都能解开看。
//! 这句话的 hint 挂在两条解码动作上，用户点之前就该看到，
//! 而不是解完才发现里面是自己的手机号。

use base64::Engine;
use serde_json::Value;

const URLSAFE: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE;
const STD: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;

/// 拆成三段。段数为 0 时给出可操作的错误
fn segments(input: &str) -> Result<Vec<&str>, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err("内容为空".to_string());
    }
    let parts: Vec<&str> = s.split('.').collect();
    match parts.len() {
        3 => Ok(parts),
        // 「不是 JWT」是这里最可能的真相（用户多半粘了个 URL 或
        // 一段代码），所以明说比报段数对不上有用
        _ => Err(format!(
            "不是 JWT：应该有三段用 . 分隔，现在是 {}{}",
            parts.len(),
            // 一段说明粘错东西了，两段说明中间被截断了。
            // 后者更可能是「从聊天软件里分段粘的」，提示值得分开
            hint_for_segment_count(parts.len())
        )),
    }
}

fn hint_for_segment_count(n: usize) -> &'static str {
    match n {
        1 => "（粘进来的可能不是 token）",
        2 => "（多半是中途被截断了，检查一下有没有分段粘）",
        _ => "（段数不对）",
    }
}

fn decode_segment(seg: &str) -> Result<Value, String> {
    // JWT 规范用 url-safe 且**允许省略填充**。用 `URL_SAFE_NO_PAD` 之外
    // 还要留 `decode_padding_mode::Indifferent` 那条路 —— 现实里
    // 相当多的实现是带 `=` 的，两种都得收
    let s = seg.trim();
    if s.is_empty() {
        return Err("有一段是空的".to_string());
    }
    let bytes = URLSAFE
        .decode(s)
        .or_else(|_| STD.decode(s))
        .or_else(|_| {
            // 手工补 `= 再解。有些实现（尤其手写的）会把填充也去掉
            // 且尾部长度是 2 或 3，而 URL_SAFE 对这两种长度是接受的，
            // 只有 1 才报 InvalidLength
            let pad = (4 - s.len() % 4) % 4;
            URLSAFE.decode(format!("{s}{}", "=".repeat(pad)))
        })
        .map_err(|e| format!("这一段不是合法 base64url（{e}）—— 可能不是 JWT"))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| "解出来不是合法 UTF-8 —— JWT 的段应当是 JSON 文本".to_string())?;
    serde_json::from_str(&text).map_err(|e| {
        format!(
            "这一段不是合法 JSON（{}）—— 可能不是 JWT",
            first_line(&e.to_string())
        )
    })
}

fn pretty(v: &Value) -> Result<String, String> {
    serde_json::to_string_pretty(v).map_err(|e| format!("序列化失败：{e}"))
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or(s).trim().to_string()
}

/// 解出 header
pub fn decode_header(input: &str) -> Result<String, String> {
    let parts = segments(input)?;
    pretty(&decode_segment(parts[0])?)
}

/// 解出 payload
pub fn decode_payload(input: &str) -> Result<String, String> {
    let parts = segments(input)?;
    pretty(&decode_segment(parts[1])?)
}

/// 读 payload 的 `exp`，判断相对现在是否过期
///
/// **注意单位。** JWT 的 `exp` 是 **Unix 秒**，而这个应用里
/// `created_at` / `now_ms()` 都是**毫秒**。拿秒去比毫秒时钟差
/// 1000 倍，症状是所有 token 都显示「1970 年就过期了」。
/// 这个坑值得单独说，因为它在测试里极容易写反方向：造一个
/// 「10 秒前」的毫秒时间戳，指望实现能认出「这明显是毫秒」——
/// 那测不出真实风险。真实的样本是**用秒**、且在**未来**。
pub fn verify_exp(input: &str) -> Result<String, String> {
    let parts = segments(input)?;
    let payload = decode_segment(parts[1])?;
    let exp = payload
        .get("exp")
        .and_then(Value::as_i64)
        .ok_or_else(|| "payload 里没有 exp 字段 —— 这个 token 不会过期".to_string())?;

    let delta = exp - unix_secs();
    let at = chrono_like(exp);
    // 三档。「10 秒后过期」说成「刚刚过期」是把方向搞反了 ——
    // 那是最容易让人误判的一档：token 其实还能用
    if delta >= 0 {
        Ok(format!("还有 {} 过期（UTC {at}）", human_delta(delta)))
    } else if delta > -60 {
        // 刚过期的与过了一个小时的对用户是两件事，都说「已过期」
        // 会把前者说得像后者那么严重
        Ok(format!("刚刚过期，不满 1 分钟（UTC {at}）"))
    } else {
        Ok(format!("已过期 {}（UTC {at}）", human_delta(-delta)))
    }
}

/// 当前 Unix 秒。抽出来是为了让单测能替换
fn unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn chrono_like(secs: i64) -> String {
    // 不引 chrono 依赖：只需要一个大致可读的时间。
    // 完整日期对「还有多久过期」这个判断没用，UTC 挂钟时间够了
    let d = secs.rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", d / 3600, (d % 3600) / 60, d % 60)
}

fn human_delta(secs: i64) -> String {
    match secs {
        s if s < 60 => format!("{s} 秒"),
        s if s < 3600 => format!("{} 分", s / 60),
        s if s < 86_400 => format!("{} 小时", s / 3600),
        s if s < 2_592_000 => format!("{} 天", s / 86_400),
        s => format!("{} 个月", s / 2_592_000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // header {"alg":"HS256","typ":"JWT"}
    // payload {"sub":"1234","name":"dev","exp":4102444800}
    const JWT: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.\
                        eyJzdWIiOiIxMjM0IiwibmFtZSI6ImRldiIsImV4cCI6NDEwMjQ0NDgwMH0.\
                        SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";

    #[test]
    fn decodes_header_pretty() {
        let out = decode_header(JWT).unwrap();
        assert!(out.contains("HS256"), "{out}");
        // 要 pretty 的：解出来是给人读的，压成一行就没意义了
        assert!(out.contains('\n'), "{out}");
    }

    #[test]
    fn decodes_payload_pretty() {
        let out = decode_payload(JWT).unwrap();
        assert!(out.contains("dev"), "{out}");
        assert!(out.contains('\n'), "{out}");
    }

    #[test]
    fn accepts_urlsafe_dash_and_underscore() {
        // 样本用 U+10FFFF：它的 UTF-8 编码是 F4 8F BF BF，
        // 6 位分组里正好落进 62 与 63 → url-safe 字母表的 - 与 _
        let payload = serde_json::json!({ "v": "\u{10FFFF}" });
        let seg = URLSAFE.encode(serde_json::to_string(&payload).unwrap());
        assert!(seg.contains('-') || seg.contains('_'), "{seg}");
        let jwt = format!("eyJhbGciOiJub25lIn0.{seg}.sig");
        let out = decode_payload(&jwt).unwrap();
        assert!(out.contains("v"), "{out}");
    }

    #[test]
    fn decodes_standard_alphabet_segments() {
        // 规范说该用 url-safe，但带 `+` `/` `=` 的实现现实里不少。
        // 只认 url-safe 的话这些 token 会被判成「不是 JWT」
        let std = STD.encode(br#"{"alg":"HS256"}"#);
        let jwt = format!("{std}.a.b");
        assert!(decode_header(&jwt).unwrap().contains("HS256"));
    }

    #[test]
    fn rejects_non_base64_segment() {
        // 长度对但字符集非法的样本
        let e = decode_header("!!!.a.b").unwrap_err();
        assert!(e.contains("base64url"), "{e}");
    }

    #[test]
    fn handles_unpadded_segments() {
        // 规范里填充是可选的，现实里也确实有一堆实现不带 `=`
        let seg = URLSAFE.encode(br#"{"a":1}"#);
        let jwt = format!("eyJhbGciOiJub25lIn0.{seg}.sig");
        assert!(decode_payload(&jwt).unwrap().contains("1"));
    }

    #[test]
    fn rejects_non_json_header() {
        // base64 解得开但不是 JSON：这段东西压根不是 JWT
        let seg = URLSAFE.encode("hello world");
        let e = decode_header(&format!("{seg}.a.b")).unwrap_err();
        assert!(e.contains("可能不是 JWT"), "{e}");
    }

    #[test]
    fn rejects_empty_segment() {
        let e = decode_payload(&format!("eyJhbGciOiJub25lIn0..sig")).unwrap_err();
        assert!(e.contains("空的"), "{e}");
    }

    #[test]
    fn rejects_wrong_segment_count() {
        // 两段最可能是「中途被截断」，提示要跟「粘错了」分开
        let e = decode_payload("aaa.bbb").unwrap_err();
        assert!(e.contains("截断"), "{e}");
        let e = decode_payload("justtext").unwrap_err();
        assert!(e.contains("不是 token"), "{e}");
        assert!(decode_payload("").unwrap_err().contains("内容为空"));
    }

    #[test]
    fn verify_exp_reads_seconds_not_milliseconds() {
        // 用**秒**，且 exp 在**未来**。方向反了的话，写错的实现
        // 会说「已过期 50 年」，而这个错在「造过去的毫秒」样本上
        // 是看不出来的
        let payload = serde_json::json!({ "exp": unix_secs() + 10 });
        let jwt = format!(
            "eyJhbGciOiJub25lIn0.{}.sig",
            URLSAFE.encode(serde_json::to_string(&payload).unwrap())
        );
        let out = verify_exp(&jwt).unwrap();
        assert!(out.contains("还有"), "{out}");
        // 关键：10 秒后过期**不是**「已过期」。把方向写反的话，
        // 这条断言会通过而实现是错的，所以单独盯住
        assert!(!out.contains("过期，不满"), "{out}");

        // 过去 1 小时 → 说「已过期 1 小时」
        let payload = serde_json::json!({ "exp": unix_secs() - 3600 });
        let jwt = format!(
            "eyJhbGciOiJub25lIn0.{}.sig",
            URLSAFE.encode(serde_json::to_string(&payload).unwrap())
        );
        let out = verify_exp(&jwt).unwrap();
        assert!(out.contains("已过期"), "{out}");
        assert!(out.contains("1 小时"), "{out}");

        // 刚过期 30 秒 → 不该说成「已过期 30 秒」那么笼统，
        // 也不该说成「已过期 1 小时」
        let payload = serde_json::json!({ "exp": unix_secs() - 30 });
        let jwt = format!(
            "eyJhbGciOiJub25lIn0.{}.sig",
            URLSAFE.encode(serde_json::to_string(&payload).unwrap())
        );
        assert!(verify_exp(&jwt).unwrap().contains("刚刚过期"));
    }

    #[test]
    fn verify_exp_without_exp_field() {
        // 「不会过期」要说清，别让用户以为是自己看漏了
        let payload = serde_json::json!({ "sub": "1" });
        let jwt = format!(
            "eyJhbGciOiJub25lIn0.{}.sig",
            URLSAFE.encode(serde_json::to_string(&payload).unwrap())
        );
        let e = verify_exp(&jwt).unwrap_err();
        assert!(e.contains("不会过期"), "{e}");
    }

    #[test]
    fn human_delta_keeps_small_values_visible() {
        // 30 秒说成「0 分」就等于没说话
        assert_eq!(human_delta(30), "30 秒");
        assert_eq!(human_delta(3600), "1 小时");
        assert_eq!(human_delta(86_400), "1 天");
    }
}
