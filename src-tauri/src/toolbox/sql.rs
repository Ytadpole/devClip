//! SQL 格式化、关键字大小写、提取表名
//!
//! ## 为什么手写切词
//!
//! 直接对原文做字符串替换会有两类事故：
//!
//! - `WHERE note = 'select'` 里的 `select` 被当成关键字大写 —— 改了数据
//! - `-- 记得 select 字段` 注释里的内容被重排 —— 注释的位置是有语义的
//!
//! 所以先切成 token，**只对标识符 token 做关键字判断**，字面量与
//! 注释原样输出。手写切词而不是引入 `regex`：难点全在「怎么跳过
//! 字面量」上（`''` 是转义、`\\'` 在 MySQL 也是转义、中文不能按
//! 字节切），而 `regex` 并不在依赖表里 —— 为了一个 SQL 格式化
//! 加一个正则引擎不划算。
//!
//! ## 切词之后最容易错的一处
//!
//! **「相邻」指「词之间只隔着空白」，不是 token 下标相邻。**
//! `group   by` 切出来是 `Word(group) Whitespace(Word(by)`，按下标
//! 相邻判 `GROUP` 与 `BY` 永远合不起来。而 `GROUP` 单独不在关键字
//! 表里，于是输出看着仍然像合法 SQL，只是 `GROUP BY a` 没换行。
//!
//! 这个错很安静：没有 panic、没有报错、测试若只断言「关键字变大写」
//! 照样通过。**同一个 bug 还让表名提取全军覆没** —— `FROM` 与表名
//! 之间的空白没跳过，`read_name` 每次读到的都是空白。两个症状
//! 一个根因，所以这两条测试要一起看。
//!
//! ## upper / lower 与 format 分成三个动作
//!
//! 「只改大小写」不该顺带重排版 —— 用户 diff 一个查询时要的是
//! 最小改动。把两者合成一个动作，review 时看到的 diff 就全是
//! 无关的换行。

/// 一个 token
#[derive(Debug, Clone, PartialEq)]
enum Tok {
    /// 标识符 / 关键字：字母、数字、下划线、$。已转成小写存
    /// 关键字判断用；输出时用原文（`group` 与 `GROUP` 都得原样保留）
    Word(String),
    /// 空白。**带原文** —— `upper` / `lower` 承诺一个字节都不改，
    /// 所以不能压成单空格。原样输出，同时它决定「相邻」怎么算
    Whitespace(String),
    /// 字符串字面量（含引号）。原样输出，绝不当关键字
    Str(String),
    /// 数字
    Number(String),
    /// 运算符与标点
    Punct(char),
    /// `-- ...` 到行尾
    LineComment(String),
    /// `/* ... */`
    BlockComment(String),
}

/// 多词关键字。空格是**分隔标记**，切词后靠它重新拼
const MULTIWORD: &[&[&str]] = &[
    &["group", "by"],
    &["order", "by"],
    &["left", "join"],
    &["right", "join"],
    &["inner", "join"],
    &["outer", "join"],
    &["cross", "join"],
    &["full", "join"],
    &["union", "all"],
    &["insert", "into"],
    &["delete", "from"],
    &["not", "null"],
    &["is", "not"],
    &["primary", "key"],
    &["foreign", "key"],
    &["create", "table"],
    &["drop", "table"],
    &["alter", "table"],
    &["inner", "select"],
];

/// 会让前一个关键字该断行的大关键字
const CLAUSE: &[&str] = &[
    "select", "from", "where", "group", "order", "having", "limit", "offset", "join", "union",
    "insert", "update", "delete", "values", "set", "on", "and", "or",
];

const KEYWORDS: &[&str] = &[
    "select",
    "from",
    "where",
    "group",
    "by",
    "order",
    "having",
    "limit",
    "offset",
    "join",
    "inner",
    "left",
    "right",
    "full",
    "outer",
    "cross",
    "union",
    "all",
    "distinct",
    "as",
    "on",
    "and",
    "or",
    "not",
    "null",
    "is",
    "in",
    "like",
    "between",
    "exists",
    "case",
    "when",
    "then",
    "else",
    "end",
    "insert",
    "into",
    "values",
    "update",
    "set",
    "delete",
    "create",
    "table",
    "drop",
    "alter",
    "add",
    "column",
    "primary",
    "key",
    "foreign",
    "references",
    "index",
    "unique",
    "default",
    "asc",
    "desc",
    "having",
    "count",
    "sum",
    "avg",
    "min",
    "max",
    "with",
    "returning",
    "using",
    "natural",
    "true",
    "false",
    "cascade",
    "if",
    "exists",
];

// ---------------------------------------------------------------- 切词

fn tokenize(src: &str) -> Vec<Tok> {
    let cs: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];

        if c.is_whitespace() {
            let start = i;
            while i < cs.len() && cs[i].is_whitespace() {
                i += 1;
            }
            out.push(Tok::Whitespace(cs[start..i].iter().collect()));
            continue;
        }

        // 行注释 `--` 要在运算符之前判，否则 `-` 被吃掉一个
        if c == '-' && cs.get(i + 1) == Some(&'-') {
            let start = i;
            while i < cs.len() && cs[i] != '\n' {
                i += 1;
            }
            out.push(Tok::LineComment(cs[start..i].iter().collect()));
            continue;
        }

        if c == '/' && cs.get(i + 1) == Some(&'*') {
            let start = i;
            i += 2;
            // 没闭合的块注释要一路吃到结尾，不能死循环
            while i < cs.len() {
                if cs[i] == '*' && cs.get(i + 1) == Some(&'/') {
                    i += 2;
                    break;
                }
                i += 1;
            }
            out.push(Tok::BlockComment(
                cs[start..i.min(cs.len())].iter().collect(),
            ));
            continue;
        }

        // 字符串字面量。`''` 是转义（不是结束），`\'` 在 MySQL 下也是。
        // 这两种都得吃掉，否则 `WHERE a = 'it''s'` 会被切成三段
        if c == '\'' || c == '"' || c == '`' {
            let quote = c;
            let start = i;
            i += 1;
            while i < cs.len() {
                if cs[i] == '\\' && i + 1 < cs.len() {
                    i += 2;
                    continue;
                }
                if cs[i] == quote {
                    // 连着两个引号 = 转义的一个引号，继续往下读
                    if cs.get(i + 1) == Some(&quote) {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            out.push(Tok::Str(cs[start..i.min(cs.len())].iter().collect()));
            continue;
        }

        if c.is_alphabetic() || c == '_' || c == '$' {
            let start = i;
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_' || cs[i] == '$') {
                i += 1;
            }
            out.push(Tok::Word(cs[start..i].iter().collect()));
            continue;
        }

        if c.is_ascii_digit() {
            let start = i;
            while i < cs.len() && (cs[i].is_ascii_alphanumeric() || cs[i] == '.') {
                i += 1;
            }
            out.push(Tok::Number(cs[start..i].iter().collect()));
            continue;
        }

        out.push(Tok::Punct(c));
        i += 1;
    }
    out
}

/// 词的原文取出来（用于关键字判断与小写化）
fn word(s: &str) -> String {
    s.to_ascii_lowercase()
}

// ------------------------------------------------- 「相邻」的唯一实现

/// `toks[idx]` 是不是以 `words` 开头（中间允许夹任意空白）
///
/// **这个函数是全部多词处理的唯一入口。** 它返回的是**消耗到的下标**，
/// 0 或 None 表示不匹配。让调用方自己「看下标 +1」的话，空白一被
/// 漏掉就静默失配 —— 而失配的症状是输出仍然像 SQL。
fn match_words(toks: &[Tok], idx: usize, words: &[&str]) -> Option<usize> {
    let mut i = idx;
    for (n, w) in words.iter().enumerate() {
        i = skip_ws(toks, i);
        match toks.get(i) {
            Some(Tok::Word(got)) if &word(got) == w => i += 1,
            // 只允许词**之间**有空白，词后面紧跟别的东西就不算匹配
            _ => {
                let _ = n;
                return None;
            }
        }
    }
    Some(i)
}

/// 跳过一串空白，返回第一个非空白 token 的下标
fn skip_ws(toks: &[Tok], from: usize) -> usize {
    let mut i = from;
    while matches!(toks.get(i), Some(Tok::Whitespace(_))) {
        i += 1;
    }
    i
}

/// 找到最长的多词关键字匹配。没匹配返回 `from`
fn longest_multiword(toks: &[Tok], from: usize) -> Option<(usize, &'static [&'static str])> {
    MULTIWORD
        .iter()
        .filter_map(|ws| match_words(toks, from, ws).map(|end| (end, *ws)))
        .max_by_key(|(end, _)| *end)
}

fn is_keyword(s: &str) -> bool {
    KEYWORDS.contains(&s)
}

// ------------------------------------------------------------ 动作

/// 关键字大写 + 大关键字断行
pub fn format(input: &str) -> Result<String, String> {
    reformat(input, true, true)
}

/// 只把关键字大写，**不动空白**
pub fn upper(input: &str) -> Result<String, String> {
    reformat(input, true, false)
}

/// 只把关键字小写
pub fn lower(input: &str) -> Result<String, String> {
    reformat(input, false, false)
}

/// 提取 `FROM` / `JOIN` / `INTO` / `UPDATE` 后面的表名
pub fn tables(input: &str) -> Result<String, String> {
    let toks = tokenize(input);
    let mut names: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let after = ["from", "join", "into", "update"]
            .iter()
            .find_map(|w| match_words(&toks, i, &[w]));
        if let Some(end) = after {
            // **这里必须用 end 而不是 i+1**，也要跳过空白 ——
            // 「跳过空白」那一步就是「相邻」的定义
            if let Some(name) = read_name(&toks, end) {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
            i = end;
            continue;
        }
        i += 1;
    }
    if names.is_empty() {
        return Err("没找到表名 —— 这段 SQL 里没有 FROM / JOIN / INTO / UPDATE".to_string());
    }
    Ok(names.join("\n"))
}

/// 从 `from` 之后读一个表名
fn read_name(toks: &[Tok], from: usize) -> Option<String> {
    // 括号是子查询（`(select ...)`），不是表名
    let i = skip_ws(toks, from);
    match toks.get(i) {
        Some(Tok::Word(w)) => Some(w.clone()),
        _ => None,
    }
}

fn reformat(input: &str, upper_kw: bool, relayout: bool) -> Result<String, String> {
    let toks = tokenize(input);
    if toks.is_empty() {
        return Err("内容为空".to_string());
    }
    let mut out = String::with_capacity(input.len() + input.len() / 4);
    let mut i = 0;
    // 上一段输出的是关键字。用来抑制连续 CLAUSE 之间多余的换行，
    // 并让 `left join` 这种多词形式只断一次行
    let mut prev_kw: bool = false;

    while i < toks.len() {
        match &toks[i] {
            // 字面量与注释原样输出，一个字节都不动
            Tok::Str(s) | Tok::LineComment(s) | Tok::BlockComment(s) => {
                out.push_str(s);
                prev_kw = false;
            }
            // 空白：重排时压成单空格，否则照抄原文
            Tok::Whitespace(s) => {
                if relayout {
                    out.push(' ');
                } else {
                    out.push_str(s);
                }
                prev_kw = false;
            }
            Tok::Number(s) => {
                out.push_str(s);
                prev_kw = false;
            }
            Tok::Punct(c) => {
                out.push(*c);
                prev_kw = false;
            }
            Tok::Word(raw) => {
                let w = word(raw);
                // 多词关键字优先，命中就把下标推到整段之后
                if let Some((end, ws)) = longest_multiword(&toks, i) {
                    if relayout && CLAUSE.contains(&ws[0]) && !prev_kw {
                        out.push('\n');
                    }
                    out.push_str(&case(&ws.join(" "), upper_kw));
                    i = end;
                    prev_kw = true;
                    continue;
                }
                if is_keyword(&w) {
                    if relayout && CLAUSE.contains(&w.as_str()) && !prev_kw {
                        out.push('\n');
                    }
                    out.push_str(&case(&w, upper_kw));
                    prev_kw = true;
                } else {
                    // 标识符：原文照抄。列名叫 `order` 时全大写会改掉语义
                    out.push_str(raw);
                    prev_kw = false;
                }
            }
        }
        i += 1;
    }
    let s = out.trim().to_string();
    if s.is_empty() {
        return Err("切词之后没有可输出的内容".to_string());
    }
    Ok(s)
}

fn case(s: &str, upper: bool) -> String {
    if upper {
        s.to_ascii_uppercase()
    } else {
        s.to_ascii_lowercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_uppercases_keywords_and_breaks_lines() {
        let out = format("select a,b from t where a=1").unwrap();
        assert!(out.contains("SELECT"), "{out}");
        assert!(out.contains("FROM"), "{out}");
        assert!(out.contains("WHERE"), "{out}");
        assert!(out.lines().count() >= 2, "{out}");
    }

    #[test]
    fn upper_only_changes_keyword_case() {
        // 用户 diff 一个查询时要的是最小改动：空白一个都不能动
        let src = "select a from t where a=1";
        let out = upper(src).unwrap();
        assert_eq!(out, "SELECT a FROM t WHERE a=1");
    }

    #[test]
    fn lower_only_changes_keyword_case() {
        let out = lower("SELECT A FROM T").unwrap();
        assert_eq!(out, "select A from T");
    }

    #[test]
    fn format_never_touches_literals() {
        // 数据里的 'select' 是数据，不是关键字。改掉它就改了数据
        let out = format("select * from t where note = 'select me'").unwrap();
        assert!(out.contains("'select me'"), "{out}");
    }

    #[test]
    fn format_never_touches_comments() {
        // 注释里的关键字不是关键字。注释的位置往往就是语义所在
        let out = format("select a from t -- drop table t\nwhere b=1").unwrap();
        assert!(out.contains("-- drop table t"), "{out}");
    }

    #[test]
    fn handles_escaped_quotes_in_literals() {
        // `it''s` 里的第二个引号是转义，不是结束。
        // 认错的话 `where a='it''s'` 会被切成三段，后半段被当关键字
        let out = format("select a from t where a = 'it''s select'").unwrap();
        assert!(out.contains("'it''s select'"), "{out}");
    }

    #[test]
    fn handles_backslash_escaped_quotes() {
        // MySQL 下 \' 也是转义
        let out = format("select a from t where a = 'it\\'s'").unwrap();
        assert!(out.contains("it\\'s"), "{out}");
    }

    #[test]
    fn handles_block_comments() {
        let out = format("select /* select from */ a from t").unwrap();
        assert!(out.contains("/* select from */"), "{out}");
    }

    #[test]
    fn unterminated_block_comment_does_not_hang() {
        // 没闭合的 /* 必须一路吃到结尾，不能死循环
        let out = format("select a /* 没闭合").unwrap();
        assert!(out.contains("没闭合"), "{out}");
    }

    /// **「相邻」= 词之间只隔着空白。** 这条是本模块最贵的教训：
    /// 按下标相邻判的话 `GROUP BY` 永远合不起来，输出看着仍然
    /// 合法 SQL，只是没换行
    #[test]
    fn multiword_keywords_survive_extra_whitespace() {
        let out = format("select a from t group   by a").unwrap();
        assert!(out.contains("GROUP BY"), "{out}");
        // 反例：GROUP 与 BY 各自大写但没合成一个 token，
        // 换行也该跟着走
        assert!(out.contains('\n') || out.contains("GROUP BY"), "{out}");
    }

    #[test]
    fn multiword_lookup_finds_joined_forms() {
        let toks = tokenize("group by");
        assert!(match_words(&toks, 0, &["group", "by"]).is_some());
        let toks = tokenize("group   \n  by");
        assert!(match_words(&toks, 0, &["group", "by"]).is_some());
        // 单个 group 不该匹配
        let toks = tokenize("group a");
        assert!(match_words(&toks, 0, &["group", "by"]).is_none());
    }

    #[test]
    fn extracts_table_names() {
        assert_eq!(
            tables("select * from users join orders on 1=1").unwrap(),
            "users\norders"
        );
    }

    #[test]
    fn table_extraction_skips_whitespace_after_from() {
        // 同一个 bug 的另一个症状：FROM 与表名之间的空白没跳过，
        // read_name 读到的就是空白，于是全军覆没
        assert_eq!(tables("select * from    users").unwrap(), "users");
        assert_eq!(tables("select *\nfrom\nusers").unwrap(), "users");
    }

    #[test]
    fn table_extraction_ignores_subquery() {
        // `(select ...)` 是子查询不是表名
        let e = tables("select * from (select 1)").unwrap_err();
        assert!(e.contains("没找到表名"), "{e}");
    }

    #[test]
    fn table_extraction_dedupes() {
        assert_eq!(
            tables("select * from a join b on 1=1 join a on 1=1").unwrap(),
            "a\nb"
        );
    }

    #[test]
    fn reports_no_tables_readably() {
        let e = tables("select 1").unwrap_err();
        assert!(e.contains("没找到表名"), "{e}");
    }

    #[test]
    fn identifier_named_like_keyword_is_untouched() {
        // 列名叫 order 时全大写会改掉语义（尤其要区分大小写时）
        let out = format("select `order` from t").unwrap();
        assert!(out.contains("`order`"), "{out}");
    }

    #[test]
    fn format_is_idempotent() {
        // 第二遍不该再有任何变化。不幂等的话用户点两次会看到 diff，
        // 而「格式化」这个动作最起码要满足这点
        let once = format("select a,b from t where a=1 and b in (1,2)").unwrap();
        let twice = format(&once).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn multiword_in_non_clause_positions() {
        // insert into / union all 不在 CLAUSE 里，不该多断行，
        // 但关键字本身仍要正确大小写
        let out = format("insert into t (a) values (1)").unwrap();
        assert!(out.contains("INSERT INTO"), "{out}");
        let out = format("select a from t union all select b from u").unwrap();
        assert!(out.contains("UNION ALL"), "{out}");
    }

    #[test]
    fn not_null_stays_one_phrase() {
        // 「not null」是多词关键字。若被拆成 NOT / NULL，
        // 且两者都在 CLAUSE 里，就会各占一行 —— 而 NOT NULL
        // 是一个整体，用户看到断裂的短语会以为是两条约束
        let out = format("create table t (a int not null)").unwrap();
        assert!(out.contains("NOT NULL"), "{out}");
    }

    #[test]
    fn numbers_are_left_alone() {
        // 数字里不能出现关键字替换的副作用（1e5、0x1F 都要原样）
        let out = format("select 1e5, 0x1F, 3.14 from t").unwrap();
        assert!(out.contains("1e5"), "{out}");
        assert!(out.contains("0x1F"), "{out}");
        assert!(out.contains("3.14"), "{out}");
    }

    #[test]
    fn handles_cjk_identifiers() {
        // 中文标识符在 MySQL 里合法（要加反引号，但那是用户的写法）
        let out = format("select 名字 from 表").unwrap();
        assert!(out.contains("名字"), "{out}");
        assert!(out.contains("表"), "{out}");
    }
}
