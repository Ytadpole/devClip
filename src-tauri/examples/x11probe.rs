//! Linux 剪贴板探针 —— 阶段 6 的事实调查，不是生产代码
//!
//! 在写实现之前先把 X11 的几件事验清楚。每条都对应一个
//! 「看起来显然、实际容易错」的假设。
//!
//! 用法：
//! - `cargo run --example x11probe`              打印当前状态
//! - `cargo run --example x11probe own "文本"`   模拟别的程序复制（保持进程存活）
//! - `cargo run --example x11probe watch 20`     观察 owner 变化

#[cfg(target_os = "linux")]
use arboard::{Clipboard, SetExtLinux};
#[cfg(target_os = "linux")]
use x11rb::connection::Connection;
#[cfg(target_os = "linux")]
use x11rb::protocol::xproto::{Atom, ConnectionExt as XProtoExt, Window};
#[cfg(target_os = "linux")]
use x11rb::rust_connection::RustConnection;

/// 返回连接与默认 screen 序号。x11rb 的 connect 一次给两个东西
#[cfg(target_os = "linux")]
fn conn() -> Result<(RustConnection, usize), Box<dyn std::error::Error>> {
    RustConnection::connect(None).map_err(Into::into)
}

/// CLIPBOARD 不是 X 协议预定义的原子（PRIMARY=1、SECONDARY=2 才是），
/// 必须按名向服务器问，所以写不成 `AtomEnum::CLIPBOARD`
#[cfg(target_os = "linux")]
fn clipboard_atom(c: &RustConnection) -> Result<Atom, Box<dyn std::error::Error>> {
    Ok(c.intern_atom(false, b"CLIPBOARD")?.reply()?.atom)
}

/// 问服务器「现在谁持有 CLIPBOARD」。返回 0 表示无人持有
/// —— 剪贴板被清空，或上一个持有者进程刚退出
#[cfg(target_os = "linux")]
fn owner_of(c: &RustConnection, sel: Atom) -> Window {
    let cookie = match c.get_selection_owner(sel) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    cookie.reply().map(|r| r.owner).unwrap_or(0)
}

#[cfg(target_os = "linux")]
fn intern(c: &RustConnection, name: &str) -> Atom {
    match c.intern_atom(false, name.as_bytes()) {
        Ok(cookie) => cookie.reply().map(|r| r.atom).unwrap_or(0),
        Err(_) => 0,
    }
}

/// 读窗口名字，用来在日志里认出「谁复制了」
#[cfg(target_os = "linux")]
fn window_name(c: &RustConnection, w: Window) -> String {
    if w == 0 {
        return "<无人持有>".into();
    }
    let utf8 = intern(c, "UTF8_STRING");
    let net = intern(c, "_NET_WM_NAME");
    for atom in [net, intern(c, "WM_NAME")] {
        if atom == 0 {
            continue;
        }
        let Ok(cookie) = c.get_property(false, w, atom, utf8, 0, 256) else {
            continue;
        };
        let Ok(r) = cookie.reply() else { continue };
        if r.value.is_empty() {
            continue;
        }
        let s: String = r
            .value
            .iter()
            .filter(|b| **b != 0)
            .map(|b| *b as char)
            .collect();
        if !s.is_empty() {
            return format!("{s} (0x{w:x})");
        }
    }
    format!("0x{w:x}")
}

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (c, screen) = conn()?;
    let root = c.setup().roots[screen].root;
    let sel = clipboard_atom(&c)?;

    let exts: Vec<String> = match c.list_extensions() {
        Ok(cookie) => cookie
            .reply()
            .map(|r| {
                r.names
                    .iter()
                    .map(|n| String::from_utf8_lossy(&n.name).to_string())
                    .collect()
            })
            .unwrap_or_default(),
        Err(_) => vec![],
    };

    println!(
        "DISPLAY        = {}",
        std::env::var("DISPLAY").unwrap_or_default()
    );
    println!("X11 root       = 0x{root:x}");
    println!("CLIPBOARD atom = {sel}");
    println!("XFIXES 扩展     = {}", exts.iter().any(|e| e == "XFIXES"));
    println!("XTEST  扩展     = {}", exts.iter().any(|e| e == "XTEST"));

    let owner = owner_of(&c, sel);
    println!("\nCLIPBOARD 持有者 = {}", window_name(&c, owner));

    match Clipboard::new() {
        Ok(mut cb) => match cb.get_text() {
            Ok(t) => {
                let preview: String = t.chars().take(60).collect();
                println!("arboard 读到     = {} 字符: {:?}", t.len(), preview);
            }
            Err(e) => println!("arboard 读失败   = {e}"),
        },
        Err(e) => println!("arboard 建连接失败 = {e}"),
    }

    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("own") => {
            let text = args.get(2).cloned().unwrap_or_else(|| "probe".into());
            // builder 式 API：set().wait().text(..)
            // wait() 的语义是「接管 CLIPBOARD 并一直应答别人的请求，
            // 直到内容被覆盖」。这正是剪贴板管理器需要的行为 ——
            // 只有这样内容才能活过源程序退出
            let mut ctx = Clipboard::new()?;
            ctx.set().wait().text(text.clone())?;
            c.flush()?;
            println!("\n已接管 CLIPBOARD，内容 = {text:?}");
            println!("现在持有者 = {}", window_name(&c, owner_of(&c, sel)));
            println!("\n等待中…… 内容被别的程序覆盖后本进程才会退出。");
            println!("对照实验：另开一个终端跑 `cargo run --example x11probe` ");
            println!("看持有者是不是这个探针进程。");
        }
        Some("watch") => {
            let n: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(20);
            println!("\n观察 CLIPBOARD 持有者变化，最多 {n} 次（每 500ms）");
            let mut last = owner_of(&c, sel);
            println!("  起点: {}", window_name(&c, last));
            let mut changes = 0;
            for i in 0..n {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let now = owner_of(&c, sel);
                if now != last {
                    changes += 1;
                    println!(
                        "  变化 {changes}（第 {} 轮）: {} -> {}",
                        i,
                        window_name(&c, last),
                        window_name(&c, now)
                    );
                    last = now;
                }
            }
            println!("观察结束，共 {changes} 次变化");
        }
        Some("active") => {
            // 逐字段打印，定位 frontmost_app() 为什么返回 None
            let root = c.setup().roots[screen].root;
            let active = intern(&c, "_NET_ACTIVE_WINDOW");
            let watom: Atom = x11rb::protocol::xproto::AtomEnum::WINDOW.into();
            println!("root=0x{root:x} _NET_ACTIVE_WINDOW atom={active} WINDOW atom={watom}");
            let cookie = c.get_property(false, root, active, watom, 0, 1).unwrap();
            let r = cookie.reply().unwrap();
            println!(
                "回复: value={:?} format={} len={}",
                r.value, r.format, r.value_len
            );
            if r.value.len() >= 4 {
                let win = u32::from_be_bytes([r.value[0], r.value[1], r.value[2], r.value[3]]);
                println!("窗口 = 0x{win:x}");
                println!("window_name = {}", window_name(&c, win));
            }
        }
        _ => println!("\n用法：x11probe [own <文本> | watch <次数> | active]"),
    }
    Ok(())
}

/// 非 Linux 目标上整文件失效，但 example 必须有 main 才能过编译
#[cfg(not(target_os = "linux"))]
fn main() {}
