//! URL 变换
//!
//! 只有两个纯字符串动作。docs/04 还列了「Open in Browser」，它需要
//! `AppHandle` 才能开浏览器 —— 而注册表里的 `run` 一律不吃应用句柄
//! （理由见 mod.rs 的模块文档）。所以它不进注册表，将来要做得单独
//! 开一个 command。

/// 去掉 `?` 及其后的一切，**保留 `#fragment`**
///
/// fragment 要留：它不是发给服务器的，不算「泄露」的部分，而
/// SPA 的 hash 路由（`/app#/settings`）去掉就跳回首页了
pub fn strip_query(input: &str) -> Result<String, String> {
    let s = trimmed(input)?;
    let (before_frag, frag) = split_fragment(s);
    let base = match before_frag.find('?') {
        Some(i) => &before_frag[..i],
        None => before_frag,
    };
    // 末尾那个孤零零的 '?'（`/a?`）也要去掉。留着会让用户以为
    // query 被截断了，而实际上它本来就是空的
    Ok(format!("{base}{frag}"))
}

/// 提取域名。去掉 `www.` 前缀与端口
///
/// `www.` 要去：用户拿到的多半是要粘到别处用的裸域名，
/// `example.com` 比 `www.example.com` 更好用
pub fn domain(input: &str) -> Result<String, String> {
    let s = trimmed(input)?;
    let after_scheme = match s.find("://") {
        Some(i) => &s[i + 3..],
        None => s,
    };
    // 到第一个 / ? # 为止 —— 路径与 query 里出现的 '/' 很常见，
    // 按整个串找最后一个 '.' 会把 `a.example.com/b.c` 读成 b.c
    let host_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let host = &after_scheme[..host_end];

    if host.is_empty() {
        return Err("没有找到域名 —— 这串东西里没有 host".to_string());
    }
    // 带 userinfo 的（user:pass@host）要剥掉，否则密码会跟着进剪贴板
    let host = match host.rsplit_once('@') {
        Some((_, h)) => h,
        None => host,
    };
    // IPv6 的字面量整体是方括号包着的，不能按 ':' 切
    if let Some(inner) = host.strip_prefix('[') {
        return inner
            .split(']')
            .next()
            .filter(|s| !s.is_empty())
            .map(|s| format!("[{s}]"))
            .ok_or_else(|| "IPv6 字面量没有闭合的 ]".to_string());
    }
    // 端口。`example.com:8080` → `example.com`；但 `ssh://git@host:22`
    // 这种剥掉 userinfo 之后就是纯 host:port
    let host = host.rsplit_once(':').map_or(host, |(h, _)| h);
    let host = host.strip_prefix("www.").unwrap_or(host);
    if host.is_empty() {
        return Err("没有找到域名 —— 这串东西里只有端口，没有 host".to_string());
    }
    Ok(host.to_string())
}

fn split_fragment(s: &str) -> (&str, &str) {
    match s.find('#') {
        Some(i) => (&s[..i], &s[i..]),
        None => (s, ""),
    }
}

fn trimmed(input: &str) -> Result<&str, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err("内容为空".to_string());
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_query_keeping_path() {
        assert_eq!(
            strip_query("https://a.com/x?y=1&z=2").unwrap(),
            "https://a.com/x"
        );
    }

    #[test]
    fn strips_query_keeping_fragment() {
        // hash 路由是 SPA 的地址，去掉就跳回首页了
        assert_eq!(
            strip_query("https://a.com/app?tab=2#/settings").unwrap(),
            "https://a.com/app#/settings"
        );
    }

    #[test]
    fn no_query_is_unchanged() {
        assert_eq!(strip_query("https://a.com/x").unwrap(), "https://a.com/x");
    }

    #[test]
    fn strips_empty_query_marker() {
        // `/a?` 里 query 本来就是空的，留着 '?' 会让人以为被截断了
        assert_eq!(strip_query("https://a.com/a?").unwrap(), "https://a.com/a");
        assert_eq!(
            strip_query("https://a.com/a?#f").unwrap(),
            "https://a.com/a#f"
        );
    }

    #[test]
    fn extracts_domain_without_www_or_port() {
        assert_eq!(
            domain("https://www.example.com:8443/a/b?c=1").unwrap(),
            "example.com"
        );
    }

    #[test]
    fn keeps_subdomain() {
        // 只去 www.，不去别的 —— api.example.com 剥成 example.com 就是错的
        assert_eq!(
            domain("https://api.example.com/v1").unwrap(),
            "api.example.com"
        );
    }

    #[test]
    fn drops_userinfo() {
        // 带密码的 URL 剥出裸 host：密码不该跟着进剪贴板
        assert_eq!(
            domain("https://user:pw@example.com/x").unwrap(),
            "example.com"
        );
    }

    #[test]
    fn keeps_ipv6_literal_intact() {
        // IPv6 字面量整体带方括号，里面全是 ':' —— 按端口切会切出 "["，
        // 而按最后一个 '.' 找又会切出 "0" 之类的东西
        assert_eq!(
            domain("http://[2001:db8::1]:8080/x").unwrap(),
            "[2001:db8::1]"
        );
    }

    #[test]
    fn rejects_empty() {
        assert!(strip_query("   ").unwrap_err().contains("内容为空"));
        assert!(domain("").unwrap_err().contains("内容为空"));
    }

    #[test]
    fn reports_relative_paths_distinctly() {
        // 相对路径不是「域名提取失败」，得说清它压根不是 URL
        let e = domain("/just/a/path").unwrap_err();
        assert!(e.contains("没有找到域名"), "{e}");
    }

    #[test]
    fn handles_ssh_urls() {
        // ssh://git@github.com:22/repo.git：userinfo 与端口都要剥掉，
        // 但它确实有个 host，所以不该报错 —— 用户多半是想知道
        // 自己在连哪台机器
        assert_eq!(
            domain("ssh://git@github.com:22/repo.git").unwrap(),
            "github.com"
        );
        assert_eq!(
            strip_query("ssh://git@github.com:22/repo.git?x=1").unwrap(),
            "ssh://git@github.com:22/repo.git"
        );
    }
}
