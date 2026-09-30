/**
 * 内容识别器 —— 阶段 3
 *
 * 按 docs/04 的 13 条规则顺序判定，先匹配到的胜出。
 * 纯函数、无 async、无系统调用，cargo test 好写。
 */
use base64::Engine;
use serde_json::Value;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    // Image 走的是非文本剪贴板 flavor，不经过本函数（输入是 &str）。
    // 但 docs/04 把它列为 13 种类型之一，删掉会让枚举与前端
    // api.ts 的 ContentType 对不上，所以保留。
    #[allow(dead_code)]
    Image,
    Json,
    Jwt,
    Uuid,
    Ip,
    Url,
    Commit,
    Exception,
    Sql,
    Base64,
    Markdown,
    Code,
    Text,
}

impl ContentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContentType::Image => "image",
            ContentType::Json => "json",
            ContentType::Jwt => "jwt",
            ContentType::Uuid => "uuid",
            ContentType::Ip => "ip",
            ContentType::Url => "url",
            ContentType::Commit => "commit",
            ContentType::Exception => "exception",
            ContentType::Sql => "sql",
            ContentType::Base64 => "base64",
            ContentType::Markdown => "markdown",
            ContentType::Code => "code",
            ContentType::Text => "text",
        }
    }
}

impl FromStr for ContentType {
    type Err = ();

    /// 数据库里存的是字符串，反查回枚举。
    ///
    /// 认不出来时**回退到 Text** 而不是报错：这是一条只读路径
    /// （查可用动作、判断动作适不适用），为了让一条脏数据
    /// 把整个工具箱卡住不划算
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "image" => ContentType::Image,
            "json" => ContentType::Json,
            "jwt" => ContentType::Jwt,
            "uuid" => ContentType::Uuid,
            "ip" => ContentType::Ip,
            "url" => ContentType::Url,
            "commit" => ContentType::Commit,
            "exception" => ContentType::Exception,
            "sql" => ContentType::Sql,
            "base64" => ContentType::Base64,
            "markdown" => ContentType::Markdown,
            "code" => ContentType::Code,
            _ => ContentType::Text,
        })
    }
}

pub fn detect(content: &str) -> ContentType {
    // 1. image — 非文本 flavor（&str 输入下不触发，阶段 4 处理二进制）
    // 2. json
    if is_json(content) {
        return ContentType::Json;
    }
    // 3. jwt
    if is_jwt(content) {
        return ContentType::Jwt;
    }
    // 4. uuid
    if is_uuid(content) {
        return ContentType::Uuid;
    }
    // 5. ip
    if is_ip(content) {
        return ContentType::Ip;
    }
    // 6. url
    if is_url(content) {
        return ContentType::Url;
    }
    // 7. commit
    if is_commit(content) {
        return ContentType::Commit;
    }
    // 8. exception
    if is_exception(content) {
        return ContentType::Exception;
    }
    // 9. sql
    if is_sql(content) {
        return ContentType::Sql;
    }
    // 10. base64
    if is_base64(content) {
        return ContentType::Base64;
    }
    // 11. markdown
    if is_markdown(content) {
        return ContentType::Markdown;
    }
    // 12. code
    if is_code(content) {
        return ContentType::Code;
    }
    // 13. text
    ContentType::Text
}

// ── 各规则实现 ─────────────────────────────────────────────────

fn is_json(content: &str) -> bool {
    serde_json::from_str::<Value>(content.trim()).is_ok()
}

fn is_jwt(content: &str) -> bool {
    let parts: Vec<&str> = content.trim().split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    // 每段是合法 base64url
    for part in &parts {
        if !is_base64url(part) {
            return false;
        }
    }
    // header 解出 JSON 含 alg
    if let Ok(header_bytes) = base64_url_decode(parts[0]) {
        if let Ok(header_str) = String::from_utf8(header_bytes) {
            if let Ok(header_json) = serde_json::from_str::<Value>(&header_str) {
                return header_json.get("alg").is_some();
            }
        }
    }
    false
}

fn is_base64url(s: &str) -> bool {
    // base64url 的字母表是 A-Za-z0-9-_ ，不是标准 base64 的 A-Za-z0-9+/。
    // 漏掉 -_ 的话，JWT 签名段里的 '_' 会让整段判定失败。
    // 另外它省略填充符，长度不要求是 4 的倍数。
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'=')
}

fn base64_url_decode(s: &str) -> Result<Vec<u8>, base64::DecodeError> {
    // base64url 省略填充符，但 URL_SAFE engine 需要 '='
    // 补足到 4 的倍数：余 1/2/3 分别补 3/2/1 个 '='
    let rem = s.len() % 4;
    let padding = if rem == 0 { 0 } else { 4 - rem };
    let padded = format!("{}{}", s, "=".repeat(padding));
    base64::engine::general_purpose::URL_SAFE.decode(&padded)
}

fn is_uuid(content: &str) -> bool {
    let s = content.trim();
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 5 {
        return false;
    }
    let lengths = [8, 4, 4, 4, 12];
    for (i, part) in parts.iter().enumerate() {
        if part.len() != lengths[i] {
            return false;
        }
        if !part.bytes().all(|b| b.is_ascii_hexdigit()) {
            return false;
        }
    }
    true
}

fn is_ip(content: &str) -> bool {
    use std::net::IpAddr;
    content.trim().parse::<IpAddr>().is_ok()
}

fn is_url(content: &str) -> bool {
    let s = content.trim();
    // git@github.com:user/repo.git 单独匹配
    if is_ssh_url(s) {
        return true;
    }
    match url::Url::parse(s) {
        Ok(u) => matches!(
            u.scheme(),
            "http" | "https" | "ftp" | "ws" | "wss" | "file" | "git"
        ),
        Err(_) => false,
    }
}

fn is_ssh_url(s: &str) -> bool {
    s.contains('@') && s.contains(':') && !s.contains("//")
}

fn is_commit(content: &str) -> bool {
    let s = content.trim();
    if s.len() < 7 || s.len() > 40 {
        return false;
    }
    if !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    // 排除纯数字（可能是订单号）
    if s.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    true
}

fn is_exception(content: &str) -> bool {
    for line in content.lines() {
        let l = line.trim();
        // ^[\w.$]+(Exception|Error)\b
        if has_exception_class(l) {
            return true;
        }
        // at xxx(Method.java:123)
        if l.starts_with("at ") && l.contains(".java:") {
            return true;
        }
        // Traceback (most recent call last)
        if l.contains("Traceback (most recent call last)") {
            return true;
        }
    }
    false
}

fn has_exception_class(line: &str) -> bool {
    // 两种形态都要认：
    //   完整限定名  java.lang.NullPointerException: msg
    //   裸类名      RuntimeError: msg
    // 裸类名光有 "Exception happened" 会误判，所以要求冒号后带
    // 空白（也就是有 message），"Exception:no-space" 这类不算。
    let head = line.split(':').next().unwrap_or("").trim();
    if !head.ends_with("Exception") && !head.ends_with("Error") {
        return false;
    }
    // 只允许包名分隔符与内部类标记
    if !head
        .chars()
        .all(|c| c.is_alphanumeric() || c == '.' || c == '$' || c == '_')
    {
        return false;
    }
    let has_message = line
        .split_once(':')
        .map(|(_, rest)| rest.starts_with(char::is_whitespace))
        .unwrap_or(false);
    head.contains('.') || has_message
}

fn is_sql(content: &str) -> bool {
    // DDL 例外：CREATE TABLE / ALTER TABLE 单独就足以定性，
    // 不像 SELECT 那样还需要第二个关键字佐证。
    //
    // 必须匹配行首（允许前导空白）。用 contains 会把
    // "we will create table later" 这类散文吞进来 —— 那是实测踩到的。
    const DDL: &[&str] = &[
        "CREATE TABLE",
        "CREATE INDEX",
        "CREATE VIEW",
        "ALTER TABLE",
        "DROP TABLE",
    ];
    if content.lines().any(|l| {
        DDL.iter()
            .any(|kw| l.trim_start().to_uppercase().starts_with(kw))
    }) {
        return true;
    }
    let upper = content.to_uppercase();

    const KEYWORDS: &[&str] = &[
        "SELECT",
        "FROM",
        "WHERE",
        "JOIN",
        "INSERT",
        "INTO",
        "UPDATE",
        "DELETE",
        "VALUES",
        "GROUP BY",
        "ORDER BY",
        "HAVING",
        "LIMIT",
        "DROP",
        "TRUNCATE",
        "UNION",
        "INNER JOIN",
        "LEFT JOIN",
        "RIGHT JOIN",
    ];
    KEYWORDS.iter().filter(|&&kw| upper.contains(kw)).count() >= 2
}

fn is_base64(content: &str) -> bool {
    let s = content.trim();
    if s.len() < 8 || !s.len().is_multiple_of(4) {
        return false;
    }
    if !s
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=')
    {
        return false;
    }
    // 排除纯数字和纯十六进制
    if s.bytes().all(|b| b.is_ascii_digit()) || s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    // 解码后 UTF-8 且可打印
    match base64::engine::general_purpose::STANDARD.decode(s) {
        Ok(bytes) => String::from_utf8(bytes)
            .map(|decoded| {
                decoded
                    .chars()
                    .all(|c| !c.is_control() || c == '\n' || c == '\r' || c == '\t')
            })
            .unwrap_or(false),
        Err(_) => false,
    }
}

fn is_markdown(content: &str) -> bool {
    // 围栏代码块本身是 markdown 的强特征，单独一个就定性。
    // 它和 code 的区别就在这对 ```，所以不能要求再凑第二类。
    if content.contains("```") {
        return true;
    }
    let mut count = 0;
    // 行首 #{1,6} + 空格
    if content.lines().any(|l| {
        let t = l.trim_start();
        let hashes = t.chars().take_while(|c| *c == '#').count();
        (1..=6).contains(&hashes) && t.chars().nth(hashes) == Some(' ')
    }) {
        count += 1;
    }
    // ]( 链接
    if content.contains("](") {
        count += 1;
    }
    // - [ ] 任务项
    if content.contains("- [ ]") || content.contains("- [x]") {
        count += 1;
    }
    count >= 2
}

fn is_code(content: &str) -> bool {
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() < 3 {
        return false;
    }
    // 存在 2 空格以上缩进
    let has_indent = lines
        .iter()
        .any(|l| l.starts_with("  ") || l.starts_with('\t'));
    // 命中语言关键字
    let has_keyword = [
        "function", "def ", "class ", "import ", "const ", "let ", "var ", "return ", "if ",
        "for ", "while ", "fn ", "pub ", "impl ", "#include", "using ",
    ]
    .iter()
    .any(|&kw| content.contains(kw));
    // { } ; 配对
    let has_braces = content.contains('{') && content.contains('}');
    let has_semicolon = content.contains(';');

    has_indent || has_keyword || (has_braces && has_semicolon)
}

// ── 测试 ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // JSON
    #[test]
    fn json_object() {
        assert_eq!(detect(r#"{"name":"andy","age":18}"#), ContentType::Json);
    }
    #[test]
    fn json_array() {
        assert_eq!(detect(r#"[1,2,3]"#), ContentType::Json);
    }
    #[test]
    fn json_nested() {
        assert_eq!(detect(r#"{"a":{"b":[1,2]}}"#), ContentType::Json);
    }
    #[test]
    fn json_not_json() {
        assert_ne!(detect(r#"{"name":"andy""#), ContentType::Json);
    }
    #[test]
    fn json_plain_text_not_json() {
        assert_ne!(detect("hello world"), ContentType::Json);
    }

    // JWT
    #[test]
    fn jwt_valid() {
        let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        assert_eq!(detect(jwt), ContentType::Jwt);
    }
    #[test]
    fn jwt_minimal() {
        let header = base64::engine::general_purpose::URL_SAFE.encode(r#"{"alg":"HS256"}"#);
        let payload = base64::engine::general_purpose::URL_SAFE.encode(r#"{"sub":"123"}"#);
        let jwt = format!("{header}.{payload}.sig");
        assert_eq!(detect(&jwt), ContentType::Jwt);
    }
    #[test]
    fn jwt_with_url_safe_chars() {
        // base64url 的 '-' 和 '_' 是签名段的常见字符。
        // 曾经的 bug：字符集误用标准 base64 的 '+' '/'，含 '_' 的签名全被拒。
        let header =
            base64::engine::general_purpose::URL_SAFE.encode(br#"{"alg":"RS256","kid":"a-b_c"}"#);
        let payload = base64::engine::general_purpose::URL_SAFE.encode(r#"{"sub":"42"}"#);
        let sig = base64::engine::general_purpose::URL_SAFE.encode(b"\xfb\xff\xfe-_\x00raw");
        let jwt = format!("{header}.{payload}.{sig}");
        assert!(jwt.contains('-') || jwt.contains('_'), "样本应含 - 或 _");
        assert_eq!(detect(&jwt), ContentType::Jwt);
    }

    #[test]
    fn jwt_two_segments_not_jwt() {
        assert_ne!(detect("abc.def"), ContentType::Jwt);
    }
    #[test]
    fn jwt_three_segments_no_alg_not_jwt() {
        let header = base64::engine::general_purpose::URL_SAFE.encode(r#"{"foo":"bar"}"#);
        let payload = base64::engine::general_purpose::URL_SAFE.encode(r#"{"baz":"qux"}"#);
        let jwt = format!("{header}.{payload}.sig");
        assert_ne!(detect(&jwt), ContentType::Jwt);
    }
    #[test]
    fn jwt_plain_text_not_jwt() {
        assert_ne!(detect("hello.world.foo"), ContentType::Jwt);
    }

    // UUID
    #[test]
    fn uuid_valid() {
        assert_eq!(
            detect("3f8a9b2c-1d4e-4f6a-8b7c-2e5d9f0a3b1c"),
            ContentType::Uuid
        );
    }
    #[test]
    fn uuid_uppercase() {
        assert_eq!(
            detect("3F8A9B2C-1D4E-4F6A-8B7C-2E5D9F0A3B1C"),
            ContentType::Uuid
        );
    }
    #[test]
    fn uuid_no_dashes_not_uuid() {
        assert_ne!(
            detect("3f8a9b2c1d4e4f6a8b7c2e5d9f0a3b1c"),
            ContentType::Uuid
        );
    }
    #[test]
    fn uuid_wrong_length_not_uuid() {
        assert_ne!(detect("3f8a9b2c-1d4e-4f6a-8b7c"), ContentType::Uuid);
    }
    #[test]
    fn uuid_plain_text_not_uuid() {
        assert_ne!(detect("hello-world-foo"), ContentType::Uuid);
    }

    // IP
    #[test]
    fn ip_v4() {
        assert_eq!(detect("192.168.1.1"), ContentType::Ip);
    }
    #[test]
    fn ip_v6() {
        assert_eq!(detect("::1"), ContentType::Ip);
    }
    #[test]
    fn ip_v6_full() {
        assert_eq!(
            detect("2001:0db8:85a3:0000:0000:8a2e:0370:7334"),
            ContentType::Ip
        );
    }
    #[test]
    fn ip_not_ip() {
        assert_ne!(detect("999.999.999.999"), ContentType::Ip);
    }
    #[test]
    fn ip_plain_text_not_ip() {
        assert_ne!(detect("hello.world"), ContentType::Ip);
    }

    // URL
    #[test]
    fn url_https() {
        assert_eq!(
            detect("https://github.com/tauri-apps/tauri"),
            ContentType::Url
        );
    }
    #[test]
    fn url_http() {
        assert_eq!(detect("http://example.com"), ContentType::Url);
    }
    #[test]
    fn url_ssh_git() {
        assert_eq!(detect("git@github.com:user/repo.git"), ContentType::Url);
    }
    #[test]
    fn url_no_scheme_not_url() {
        assert_ne!(detect("example.com/path"), ContentType::Url);
    }
    #[test]
    fn url_plain_text_not_url() {
        assert_ne!(detect("hello world"), ContentType::Url);
    }

    // Commit
    #[test]
    fn commit_sha() {
        assert_eq!(
            detect("a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0"),
            ContentType::Commit
        );
    }
    #[test]
    fn commit_short_sha() {
        assert_eq!(detect("a1b2c3d"), ContentType::Commit);
    }
    #[test]
    fn commit_pure_digits_not_commit() {
        assert_ne!(detect("1234567"), ContentType::Commit);
    }
    #[test]
    fn commit_too_short_not_commit() {
        assert_ne!(detect("a1b2c"), ContentType::Commit);
    }
    #[test]
    fn commit_plain_text_not_commit() {
        assert_ne!(detect("abcdefg"), ContentType::Commit);
    }

    // Exception
    #[test]
    fn exception_java() {
        let ex = "java.lang.NullPointerException: Something went wrong\n\tat com.example.Foo.bar(Foo.java:42)";
        assert_eq!(detect(ex), ContentType::Exception);
    }
    #[test]
    fn exception_python() {
        let ex = "Traceback (most recent call last):\n  File \"foo.py\", line 1\nValueError: bad";
        assert_eq!(detect(ex), ContentType::Exception);
    }
    #[test]
    fn exception_error_class() {
        assert_eq!(
            detect("java.io.IOException: file not found"),
            ContentType::Exception
        );
    }
    #[test]
    fn exception_plain_text_not_exception() {
        assert_ne!(detect("this is an error message"), ContentType::Exception);
    }
    #[test]
    fn exception_single_word_not_exception() {
        assert_ne!(
            detect("NullPointerException happened"),
            ContentType::Exception
        );
    }
    // 裸类名要带 ": message" 才算，挡住散文里的 Exception
    #[test]
    fn exception_bare_class_with_message() {
        assert_eq!(
            detect("RuntimeError: division by zero"),
            ContentType::Exception
        );
        assert_eq!(detect("ValueError: bad input"), ContentType::Exception);
    }
    #[test]
    fn exception_word_in_prose_not_exception() {
        assert_ne!(
            detect("there was an Exception yesterday"),
            ContentType::Exception
        );
        assert_ne!(detect("Error:no space after colon"), ContentType::Exception);
    }

    // SQL
    #[test]
    fn sql_select_from() {
        assert_eq!(detect("SELECT * FROM users WHERE id = 1"), ContentType::Sql);
    }
    #[test]
    fn sql_insert() {
        assert_eq!(
            detect("INSERT INTO users (name) VALUES ('andy')"),
            ContentType::Sql
        );
    }
    #[test]
    fn sql_lowercase() {
        assert_eq!(
            detect("select name from users where age > 18"),
            ContentType::Sql
        );
    }
    #[test]
    fn sql_single_keyword_not_sql() {
        assert_ne!(detect("select something"), ContentType::Sql);
    }
    #[test]
    fn sql_plain_text_not_sql() {
        assert_ne!(detect("hello from the other side"), ContentType::Sql);
    }
    // DDL 现在是单关键字即命中，这里守住它不会反过来把散文吃掉
    #[test]
    fn sql_ddl_alone_is_enough() {
        assert_eq!(detect("CREATE TABLE t (id INT)"), ContentType::Sql);
        assert_eq!(detect("ALTER TABLE t ADD COLUMN c INT"), ContentType::Sql);
    }
    #[test]
    fn sql_ddl_word_in_prose_not_sql() {
        assert_ne!(detect("we will create table later"), ContentType::Sql);
    }

    // Base64
    #[test]
    fn base64_valid() {
        assert_eq!(detect("aGVsbG8gd29ybGQ="), ContentType::Base64);
    }
    #[test]
    fn base64_url_safe() {
        // aGVsbG8td29ybGQ= 是 "hello-world" 的 base64url 编码
        assert_eq!(detect("aGVsbG8td29ybGQ="), ContentType::Base64);
    }
    #[test]
    fn base64_wrong_length_not_base64() {
        assert_ne!(detect("aGVsbG8"), ContentType::Base64);
    }
    #[test]
    fn base64_pure_digits_not_base64() {
        assert_ne!(detect("12345678"), ContentType::Base64);
    }
    #[test]
    fn base64_plain_text_not_base64() {
        assert_ne!(detect("hello world"), ContentType::Base64);
    }

    // Markdown
    #[test]
    fn markdown_heading_and_list() {
        let md = "# Title\n\n- [ ] task one\n- [x] task two";
        assert_eq!(detect(md), ContentType::Markdown);
    }
    #[test]
    fn markdown_code_block_and_link() {
        let md = "```rust\nfn main() {}\n```\n\n[link](https://example.com)";
        assert_eq!(detect(md), ContentType::Markdown);
    }
    #[test]
    fn markdown_single_element_not_markdown() {
        assert_ne!(detect("# Just a heading"), ContentType::Markdown);
    }
    #[test]
    fn markdown_plain_text_not_markdown() {
        assert_ne!(detect("hello world"), ContentType::Markdown);
    }
    #[test]
    fn markdown_single_bold_not_markdown() {
        assert_ne!(detect("**bold**"), ContentType::Markdown);
    }
    // 围栏代码块现在是单特征即命中（它和 code 就靠这对 ``` 区分）
    #[test]
    fn markdown_fenced_block_alone() {
        assert_eq!(detect("```\nplain text\n```"), ContentType::Markdown);
    }

    // Code
    #[test]
    fn code_indented() {
        let code = "fn main() {\n    println!(\"hello\");\n}";
        assert_eq!(detect(code), ContentType::Code);
    }
    #[test]
    fn code_with_braces_semicolons() {
        let code = "if (x > 0) {\n  return x;\n}";
        assert_eq!(detect(code), ContentType::Code);
    }
    #[test]
    fn code_python() {
        let code = "def hello():\n    print(\"world\")\n    return 42";
        assert_eq!(detect(code), ContentType::Code);
    }
    #[test]
    fn code_two_lines_not_code() {
        assert_ne!(detect("line one\nline two"), ContentType::Code);
    }
    #[test]
    fn code_plain_text_not_code() {
        assert_ne!(detect("hello\nworld\nfoo"), ContentType::Code);
    }

    // Text
    #[test]
    fn text_plain() {
        assert_eq!(detect("hello world"), ContentType::Text);
    }
    #[test]
    fn text_chinese() {
        assert_eq!(detect("你好世界"), ContentType::Text);
    }
    #[test]
    fn text_number() {
        // 裸数字是合法 JSON，会被识别为 Json
        assert_eq!(detect("42"), ContentType::Json);
    }
    #[test]
    fn text_empty() {
        assert_eq!(detect(""), ContentType::Text);
    }
    #[test]
    fn text_with_punctuation() {
        assert_eq!(detect("Hello, world! How are you?"), ContentType::Text);
    }
}

/// 精度验收（docs/06 要求 ≥ 95%）。
/// 单独一个测试：一条错就报全部，方便看清哪些类型偏了。
#[cfg(test)]
mod accuracy {
    use super::*;

    #[test]
    fn realistic_content_accuracy() {
        let cases: Vec<(&str, &str)> = vec![
            // json 10
            ("json", r#"{"name":"andy","age":18}"#),
            ("json", "[1,2,3,4,5]"),
            ("json", r#"{"a":{"b":{"c":[1,{"d":null}]}}}"#),
            ("json", r#"{"key":"value","n":1.5,"b":true,"z":null}"#),
            ("json", r#"[{"id":1},{"id":2}]"#),
            ("json", r#"{"empty_obj":{},"empty_arr":[]}"#),
            ("json", r#"{"unicode":"中文测试","emoji":"🎉"}"#),
            ("json", r#"{"escaped":"line1\nline2\ttab"}"#),
            ("json", "42"),
            ("json", r#"{"deep":{"a":{"b":{"c":{"d":1}}}}}"#),
            // jwt 6
            ("jwt", "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkFuZHkifQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"),
            ("jwt", "eyJhbGciOiJSUzI1NiJ9.eyJpc3MiOiJqb2UiLCJleHAiOjE3MDAwMDAwMDB9.abc"),
            ("jwt", "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.sig-with_underscore"),
            ("jwt", "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.sig-with-dash"),
            ("jwt", "eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJhdWQiOiJ4In0.c2ln"),
            ("jwt", "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"),
            // uuid 6
            ("uuid", "3f8a9b2c-1d4e-4f6a-8b7c-2e5d9f0a3b1c"),
            ("uuid", "3F8A9B2C-1D4E-4F6A-8B7C-2E5D9F0A3B1C"),
            ("uuid", "00000000-0000-0000-0000-000000000000"),
            ("uuid", "ffffffff-ffff-ffff-ffff-ffffffffffff"),
            ("uuid", "a1b2c3d4-e5f6-4a7b-8c9d-0e1f2a3b4c5d"),
            ("uuid", "123e4567-e89b-12d3-a456-426614174000"),
            // ip 6
            ("ip", "192.168.1.1"),
            ("ip", "8.8.8.8"),
            ("ip", "172.16.254.3"),
            ("ip", "::1"),
            ("ip", "2001:0db8:85a3:0000:0000:8a2e:0370:7334"),
            ("ip", "fe80::1"),
            // url 8
            ("url", "https://github.com/tauri-apps/tauri"),
            ("url", "http://example.com/path?query=1&x=2"),
            ("url", "https://registry.npmmirror.com/vue"),
            ("url", "ftp://files.example.com/pub/readme.txt"),
            ("url", "ws://localhost:8080/socket"),
            ("url", "file:///home/user/notes.md"),
            ("url", "git@github.com:user/repo.git"),
            ("url", "https://api.internal.dev:8443/v1/users?page=2"),
            // commit 5
            ("commit", "a1b2c3d"),
            ("commit", "8f3e2a1b9c4d7e6f5a0b"),
            ("commit", "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0"),
            ("commit", "da39a3ee5e6b4b0d3255bfef95601890afd80709"),
            ("commit", "deadbeef"),
            // exception 8
            ("exception", "java.lang.NullPointerException: Cannot invoke \"String.length()\"\n\tat com.example.Foo.bar(Foo.java:42)"),
            ("exception", "org.springframework.dao.QueryTimeoutException: Could not execute JDBC Query\n\tat org.springframework.orm.jpa.vendor.HibernateJpaDialect.convert(HibernateJpaDialect.java:275)"),
            ("exception", "java.io.IOException: File not found"),
            ("exception", "java.lang.IllegalStateException: Bean not ready\n\tat com.devclip.service.UserRepository.findById(UserRepository.java:42)"),
            ("exception", "javax.servlet.ServletException: Request processing failed"),
            ("exception", "com.fasterxml.jackson.core.JsonParseException: Unexpected character"),
            ("exception", "Traceback (most recent call last):\n  File \"main.py\", line 3, in <module>\nValueError: invalid literal"),
            ("exception", "RuntimeError: something went very wrong"),
            // sql 10
            ("sql", "SELECT * FROM users WHERE id = 1"),
            ("sql", "select name, email from users where age > 18 order by name limit 50"),
            ("sql", "INSERT INTO orders (id, user_id) VALUES (1, 100)"),
            ("sql", "UPDATE users SET email = ? WHERE id = ?"),
            ("sql", "DELETE FROM sessions WHERE expired_at < NOW()"),
            ("sql", "CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(64))"),
            ("sql", "SELECT u.name, COUNT(o.id) FROM users u LEFT JOIN orders o ON o.user_id = u.id GROUP BY u.name"),
            ("sql", "ALTER TABLE users ADD COLUMN email VARCHAR(255)"),
            ("sql", "select * from t where a = 1 and b = 2 or c = 3"),
            ("sql", "SELECT COUNT(*) AS total FROM logs WHERE created_at > '2024-01-01' GROUP BY level HAVING COUNT(*) > 10"),
            // base64 7
            ("base64", "aGVsbG8gd29ybGQ="),
            ("base64", "eyJuYW1lIjoiYW5keSJ9"),
            ("base64", "RGV2Q2xpcCBib2NrdW1lbnQgdHlwZSBkZXRlY3Rpb24="),
            ("base64", "U3RyaW5nIGJhc2U2NCBlbmNvZGluZw=="),
            ("base64", "VGhpcyBpcyBhIHRlc3Qgc3RyaW5nIQ=="),
            ("base64", "c29tZSBkYXRhIHRoYXQgaXMgbm90IGJhc2U2NA=="),
            ("base64", "U29tZSBtb3JlIHRleHQgdGhhdCBpcyBsb25nZXI="),
            // markdown 7
            ("markdown", "# Title\n\n- [ ] task one\n- [x] task two"),
            ("markdown", "## Section\n\n[link](https://example.com)"),
            ("markdown", "```rust\nfn main() {}\n```"),
            ("markdown", "# Doc\n\nSee [README](https://github.com/a/b) for details."),
            ("markdown", "## Changelog\n\n- [x] initial commit\n- [ ] add tests"),
            ("markdown", "```python\nprint('hi')\n```\n\nRead [docs](https://x.com/y)."),
            ("markdown", "### Notes\n\n- item\n- [ ] todo"),
            // code 8
            ("code", "fn main() {\n    let x = 1;\n    println!(\"{}\", x);\n}"),
            ("code", "def hello():\n    print(\"world\")\n    return 42"),
            ("code", "public class Foo {\n    public static void main(String[] args) {\n        System.out.println(\"hi\");\n    }\n}"),
            ("code", "function add(a, b) {\n  return a + b;\n}"),
            ("code", "const x = 1;\nlet y = 2;\nif (x < y) { return true; }"),
            ("code", "import os\nimport sys\n\ndef main():\n    pass"),
            ("code", "#include <stdio.h>\nint main(void) {\n    printf(\"hi\\n\");\n    return 0;\n}"),
            ("code", "export function hello() {\n  const a = 1;\n  return a;\n}"),
            // text 5（"6379" 不在这里：裸数字是合法 JSON，会被判成 json）
            ("text", "hello world"),
            ("text", "会议室 B-1011 改到周四 10:00"),
            ("text", "192.168.1.100:5432"),
            ("text", "BEGIN RSA PRIVATE KEY-----"),
            ("text", "这是一段普通的中文说明文字，不属于任何特定格式。"),
        ];

        assert_eq!(cases.len(), 86, "样本数应与 docs/06 说的量级相当");

        let mut wrong: Vec<String> = Vec::new();
        for (want, content) in &cases {
            let got = detect(content);
            if got.as_str() != *want {
                let head: String = content.chars().take(52).collect();
                wrong.push(format!(
                    "  期望 {:<9} 实得 {:<9} | {}",
                    want,
                    got.as_str(),
                    head
                ));
            }
        }

        let acc = 100.0 * (cases.len() - wrong.len()) as f64 / cases.len() as f64;
        if !wrong.is_empty() {
            println!("误判 {} 条：\n{}", wrong.len(), wrong.join("\n"));
        }
        println!(
            "准确率 {acc:.1}% （{}/{}）",
            cases.len() - wrong.len(),
            cases.len()
        );
        assert!(acc >= 95.0, "准确率 {acc:.1}% 低于 95%");
    }
}
