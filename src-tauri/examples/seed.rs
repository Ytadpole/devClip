//! 造 1000 条测试数据并量查询性能 —— 阶段 4 验收
//!
//! 用法：cargo run --release --example seed
//! 数据落到 app data 目录的真实 devclip.db，跑完可以直接在应用里搜到。

use devclip_lib::db;
use devclip_lib::repo::{self, NewItem};
use std::path::PathBuf;

const N: usize = 1000;

fn db_path() -> PathBuf {
    // 与 lib.rs 的 db_path 一致：Linux 下是 ~/.local/share/devclip
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap()).join(".local/share"));
    base.join("com.devclip.app").join("devclip.db")
}

fn main() {
    let path = db_path();
    println!("数据库: {}", path.display());

    let conn = db::open(&path).unwrap_or_else(|e| panic!("打开失败: {e}"));
    let total_before = repo::count(&conn).unwrap();
    println!("已有 {total_before} 条，本次造 {N} 条");

    // 覆盖 13 种类型，模板要能触发 detect 的各条规则。
    // 用普通字符串而非 format!：format! 会把 {i} 当成占位符
    let templates: Vec<String> = vec![
        r#"{"service":"api-{i}","replicas":3,"region":"cn-north-1"}"#.into(),
        "SELECT id, email FROM orders_{i} WHERE status = $1 LIMIT 50".into(),
        "kubectl rollout restart deployment/api-{i} --namespace production".into(),
        "https://internal.example.com/dashboards/{i}?range=24h".into(),
        "b7d2f4a1-9c3e-4f80-8a1d-{i}e6c93b52".into(),
        "172.16.{i}.24".into(),
        "RGV2Q2xpcCBib2NrdW1lbnQge24ge249Cg==".into(),
        "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d{i}".into(),
        "会议室 B-{i} 改到周四 10:00".into(),
        "## PR #{i}\n\n- [x] 补测试\n- [ ] 合入 release".into(),
        "java.lang.IllegalStateException: bean not ready (attempt {i})\n\tat com.example.Boot.run(Boot.java:{i})".into(),
        "3f8a9b2c-1d4e-4f6a-8b7c-2e5d9f0a{i}".into(),
        "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyLW4{i}In0.sig-{i}".into(),
    ];

    let t0 = std::time::Instant::now();
    for i in 0..N {
        let tpl = &templates[i % templates.len()];
        // 序号既填进模板又留在尾部：uuid / commit 那些占位符在
        // 末尾会补不齐长度，统一靠尾部 #i 保证 hash 各不相同
        let content = format!("{}   #{}", tpl.replace("{i}", &i.to_string()), i);
        repo::upsert(
            &conn,
            &NewItem {
                content,
                source_app: Some("seed".into()),
                image_path: None,
                sensitive: false,
                expires_at: None,
            },
        )
        .unwrap_or_else(|e| panic!("第 {i} 条入库失败: {e}"));
    }
    let seed_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let total = repo::count(&conn).unwrap();
    println!("入库 {N} 条耗时 {seed_ms:.0}ms，当前共 {total} 条");

    // ── 性能验收：各类查询都要 < 50ms ──
    let cases: Vec<(&str, repo::Query)> = vec![
        (
            "子串 ser（trigram）",
            repo::Query {
                text: Some("ser".into()),
                ..Default::default()
            },
        ),
        // 搜的是内容里的真实子串，不是 id。模板按 i % 13 取模，
        // 所以 orders_1 / orders_14 / orders_27 存在，orders_42 不存在
        (
            "精确 orders_14",
            repo::Query {
                text: Some("orders_14".into()),
                ..Default::default()
            },
        ),
        (
            "子串 oduction",
            repo::Query {
                text: Some("oduction".into()),
                ..Default::default()
            },
        ),
        (
            "两字中文（LIKE 兜底）",
            repo::Query {
                text: Some("会议".into()),
                ..Default::default()
            },
        ),
        (
            "无查询全量",
            repo::Query {
                limit: Some(200),
                ..Default::default()
            },
        ),
        (
            "类型过滤 sql",
            repo::Query {
                types: Some(vec!["sql".into()]),
                limit: Some(200),
                ..Default::default()
            },
        ),
        (
            "仅收藏",
            repo::Query {
                favorite_only: Some(true),
                ..Default::default()
            },
        ),
    ];

    println!("\n{:<24} {:>10} {:>8}", "查询", "耗时", "命中");
    let mut worst = 0.0f64;
    for (name, q) in cases {
        let t = std::time::Instant::now();
        let got = repo::list(&conn, &q).unwrap();
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        worst = worst.max(ms);
        let mark = if ms < 50.0 { "✓" } else { "✗ 超验收线" };
        println!("{name:<24} {ms:>7.2}ms {:>8}  {mark}", got.len());
    }
    println!("\n最慢 {worst:.2}ms（验收线 50ms）");

    // 收藏几条，让「仅收藏」有数据可查
    let ids: Vec<i64> = repo::list(
        &conn,
        &repo::Query {
            limit: Some(5),
            ..Default::default()
        },
    )
    .unwrap()
    .iter()
    .map(|i| i.id)
    .collect();
    for &id in &ids {
        repo::toggle_favorite(&conn, id).unwrap();
    }
    println!("已收藏 {} 条，可在应用里点「仅收藏」查看", ids.len());
    println!("\n打开应用：VITE_BACKEND=tauri pnpm tauri dev");
}
