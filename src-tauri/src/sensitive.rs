//! 敏感内容识别 —— 阶段 5
//!
//! 剪贴板历史是**明文存盘**的东西。用户某天粘了个 API key 进来，
//! 半年后翻历史还能翻到，而且搜索一搜就出来 —— 这是不可接受的事。
//! docs/07 把它列成 P0。
//!
//! 只做**明确无疑**的规则。宁可漏报也不误报：把一段普通 SQL
//! 判成密钥，用户会立刻把这个功能关掉，那漏报就变成了真问题。

/// 返回命中的规则说明，`None` 表示没问题
pub fn scan(content: &str) -> Option<&'static str> {
    // 私钥块：格式固定，不可能误报
    if content.contains("-----BEGIN") && content.contains("PRIVATE KEY") {
        return Some("检测到私钥");
    }
    // AWS Access Key ID：AKIA + 16 位大写字母数字，长度固定
    if has_token(content, "AKIA", 20) {
        return Some("检测到 AWS Access Key");
    }
    // GitHub PAT：ghp_ / gho_ / ghu_ / ghs_ / ghr_ + 36 位
    for prefix in ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"] {
        if has_token(content, prefix, prefix.len() + 36) {
            return Some("检测到 GitHub Token");
        }
    }
    // Stripe：只有 live 算敏感，test 是公开的
    if has_token(content, "sk_live_", 24) {
        return Some("检测到 Stripe 密钥");
    }
    // Slack bot token
    if has_token(content, "xoxb-", 20) {
        return Some("检测到 Slack Token");
    }
    None
}

/// 找长度为 `len`、以 `prefix` 开头的 token，且前后是边界。
///
/// 边界指前后不能紧邻字母数字或下划线。少了这个判断，
/// 「mask_live_xxx」「mask_live」都会被当成真密钥 —— 而
/// `sk_live_` 出现在 URL 里（`?key=sk_live_xxx`）仍要命中，
/// 所以只看左侧边界不够
fn has_token(content: &str, prefix: &str, len: usize) -> bool {
    let bytes = content.as_bytes();
    let pb = prefix.as_bytes();
    // 三种越界都要挡。第一版只判了 `prefix.len() < len`，
    // 结果复制一个 4 字符的 "AKIA" 就 panic 在切片上 ——
    // 而这条路径跑在监听线程里，一个 panic 意味着监听整个死掉
    if len == 0 || pb.len() >= len || bytes.len() < len {
        return false;
    }
    (0..=bytes.len() - len).any(|i| {
        if &bytes[i..i + pb.len()] != pb {
            return false;
        }
        let tail_ok = bytes[i + pb.len()..i + len]
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'_');
        let head_ok = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
        tail_ok && head_ok
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_private_key() {
        assert!(scan("-----BEGIN RSA PRIVATE KEY-----\nMIIE...").is_some());
        assert!(scan("-----BEGIN OPENSSH PRIVATE KEY-----").is_some());
        assert!(scan("-----BEGIN OPENSSH PRIVATE KEY-----\nxyz\n-----END").is_some());
    }

    /// 证书不是私钥。判错的话用户导出一堆证书时会以为程序坏了
    #[test]
    fn certificate_is_not_a_private_key() {
        assert!(scan("-----BEGIN CERTIFICATE-----\nMIIDdzCC").is_none());
    }

    #[test]
    fn detects_cloud_and_service_tokens() {
        assert!(scan("AKIAIOSFODNN7EXAMPLE").is_some());
        assert!(scan(&format!("ghp_{}", "a".repeat(36))).is_some());
        assert!(scan(&format!("sk_live_{}", "b".repeat(17))).is_some());
        assert!(scan(&format!("xoxb-{}", "1".repeat(15))).is_some());
    }

    /// 长度刚好够的必须命中，不能被「宁可漏报」带偏
    #[test]
    fn flags_token_at_exact_length() {
        assert!(scan("AKIAAAAAAAAAAAAAAAAA").is_some()); // AKIA + 16
        assert!(scan("AKIAAAAAAAAAAAAAAA").is_none()); // 少一位
    }

    /// 误报比漏报更糟：把普通代码判成密钥，用户会直接关掉功能
    #[test]
    fn does_not_flag_ordinary_content() {
        for s in [
            "SELECT * FROM users WHERE id = 1",
            "sk_test_1234567890abcdef",     // 测试密钥不是真密钥
            "mask_live_1234567890abcdefgh", // 前缀嵌在词里
            "AKIA",                         // 太短
            "akiaiosfodnn7example",         // 小写
            "https://example.com/akia",     // 长度不够
            "ghp_short",                    // 不够 36 位
            "aws_secret_access_key",        // 只是提到字段名
            "MY_AKIA12345678901234567",     // AKIA 紧邻下划线，是更大 token 的一部分
            "",
        ] {
            assert!(scan(s).is_none(), "{s:?} 被误判成敏感信息");
        }
    }

    /// token 嵌在 URL 里也要命中 —— 用户经常这么粘贴
    #[test]
    fn detects_token_embedded_in_url() {
        assert!(scan("https://api.example.com/?key=AKIAIOSFODNN7EXAMPLE").is_some());
        assert!(scan(&format!("export TOKEN=ghp_{}", "z".repeat(36))).is_some());
    }

    /// 这条曾是真崩溃：复制一个 4 字符的 "AKIA" 就在切片上 panic，
    /// 而监听线程 panic 意味着监听整个死掉
    #[test]
    fn short_input_does_not_panic() {
        for n in 0..40 {
            let s = "A".repeat(n);
            let _ = scan(&s);
            let _ = scan(&format!("AKIA{s}"));
        }
    }

    /// 多行内容 —— 粘的往往是一整个配置文件
    #[test]
    fn scans_multiline_content() {
        let cfg = "# 生产环境配置\nAWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE\nAWS_SECRET=xxxx\n";
        assert!(scan(cfg).is_some());
    }
}
