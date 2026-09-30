//! Linux 监听的真机验收 —— 阶段 6
//!
//! 单元测试只能验判断逻辑，「轮询能不能真的发现复制」「接管之后
//! 内容能不能活过源程序退出」这些必须在这台机器上跑一遍。
//!
//! 用法：`cargo run --example linux_live`
//!
//! **源程序用独立进程模拟**，这是必须的：arboard 内部是进程级
//! 单例，同进程内所有 `Clipboard::new()` 共享同一个 X11 窗口，
//! 所以在同一个进程里「模拟复制」根本不会被 owner 轮询看见。
//! 那个坑我已经踩过一次了。

#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(target_os = "linux")]
use std::sync::{Arc, Mutex};
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

/// 抓到的一条：(经过的秒数, 来源窗口名, 内容)
#[cfg(target_os = "linux")]
type Capture = (f64, Option<String>, String);

/// 用独立进程模拟「用户在别的程序里复制」。子进程退出即模拟源程序关闭
#[cfg(target_os = "linux")]
fn copy_in_separate_process(text: &str) -> std::io::Result<()> {
    let mut cmd = std::process::Command::new(std::env::current_exe()?);
    cmd.arg("--copy-as-source")
        .arg(text)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd.spawn()?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn run_as_source(text: &str) -> ! {
    use arboard::SetExtLinux;
    let mut cb = arboard::Clipboard::new().expect("建剪贴板连接");
    // wait() 让本进程一直持有并应答请求，直到被人覆盖 ——
    // 这模拟一个还没退出的源程序
    let _ = cb.set().wait().text(text.to_owned());
    // 被覆盖后本进程退出，模拟「用户关掉了那个程序」
    std::process::exit(0);
}

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|a| a == "--copy-as-source") {
        let text = std::env::args().nth(2).unwrap_or_default();
        run_as_source(&text);
    }

    if let Some(why) = devclip_lib::clipboard::linux::monitor_blocker() {
        println!("不能监听：{why}");
        return Ok(());
    }

    let captured: Arc<Mutex<Vec<Capture>>> = Arc::new(Mutex::new(Vec::new()));
    let count = Arc::new(AtomicUsize::new(0));

    let sink = Arc::clone(&captured);
    let n = Arc::clone(&count);
    let start = Instant::now();
    let running = devclip_lib::clipboard::linux::spawn_watcher(move |text: String| {
        n.fetch_add(1, Ordering::SeqCst);
        let app = devclip_lib::clipboard::linux::frontmost_app();
        sink.lock()
            .unwrap()
            .push((start.elapsed().as_secs_f64(), app, text));
    })?;

    println!("=== 阶段 1：轮询能否发现别的进程的复制 ===");
    println!("监听已启动，等 2 秒让基准稳定\n");
    std::thread::sleep(Duration::from_secs(2));

    let cases = [
        ("一行 SQL", "SELECT id, email FROM users WHERE active = 1"),
        (
            "一段 JSON",
            r#"{"service":"api","replicas":3,"region":"cn-north-1"}"#,
        ),
        (
            "一个 URL",
            "https://internal.example.com/dashboards/42?range=24h",
        ),
        ("中文两字词", "会议室"),
        ("疑似密钥（应跳过）", "AKIAIOSFODNN7EXAMPLE"),
        ("最后一条 SQL", "SELECT count(*) FROM sessions"),
    ];

    for (label, text) in cases {
        copy_in_separate_process(text)?;
        println!(
            "  [+{:.1}s] 已在子进程里复制 {label}",
            start.elapsed().as_secs_f64()
        );
        // 等两个多轮询周期。兜底读是 2 秒一次，所以留够
        std::thread::sleep(Duration::from_millis(2600));
    }

    running.store(false, Ordering::SeqCst);
    let got = count.load(Ordering::SeqCst);
    println!("\n=== 阶段 2：监听到了 {got} / 5 条（密钥那条应被跳过）===\n");
    for (at, app, text) in captured.lock().unwrap().iter() {
        let preview: String = text.chars().take(44).collect();
        println!(
            "  [{at:5.1}s] 来源={:<16} {preview}",
            app.as_deref().unwrap_or("<未知>")
        );
    }

    assert_eq!(got, 5, "应监听到全部 5 条非敏感内容");
    println!("\n全部 5 条都监听到了 —— 轮询可靠，且敏感内容被拦下。");
    println!("\n=== 阶段 3：接管是否保住内容 ===");
    println!("  子进程都已退出（模拟源程序关闭）。");
    println!("  现在剪贴板归 DevClip 的接管线程所有，去别的应用按 Ctrl+V：");
    println!("    粘得出来 = 接管成功，docs/05 风险 4 的对策生效");
    println!("    粘不出来 = 接管失败");
    println!("\n按 Ctrl+C 结束。");
    while running.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(200));
    }
    std::thread::sleep(Duration::from_secs(1));
    Ok(())
}

/// 非 Linux 目标上整文件失效，但 example 必须有 main 才能过编译
#[cfg(not(target_os = "linux"))]
fn main() {}
