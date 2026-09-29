//! Linux 剪贴板监听与模拟粘贴
//!
//! ## X11 与 macOS 的机制完全不同
//!
//! macOS 有 `changeCount`，改动就 +1，轮询它即可。
//! X11 走 **Selection 机制**：没有「剪贴板」这个东西，
//! 只有一个 CLIPBOARD selection 和它的**持有者窗口**。
//! 内容存在持有者的一个 window property 里，别人来取时持有者
//! 要现场应答（`SelectionRequest`）。
//!
//! 两个直接后果：
//!
//! 1. **只能靠持有者变化判断「有没有新内容」** —— 没有计数器可轮询
//! 2. **持有者进程一死内容就没了** —— 这是 docs/05 风险 4。
//!    所以剪贴板管理器不只是「读」，还必须**接管**，
//!    自己变成持有者并一直应答别人的请求
//!
//! ## 本机验证过的事实
//!
//! 用 `cargo run --example x11probe` 在这台机器上实测过，
//! 几条容易搞错的：
//!
//! - **CLIPBOARD 不是 X 预定义的原子**（PRIMARY=1、SECONDARY=2 是），
//!   必须 `InternAtom` 按名问。写 `AtomEnum::CLIPBOARD` 编译不过
//! - **轮询 `XGetSelectionOwner` 能可靠发现复制**。500ms 间隔下
//!   实测能抓到，且不会漏掉同一窗口内的连续两次复制
//! - **arboard 的 `set().wait()` 在内容被覆盖时会自己返回**。
//!   这条很关键：它意味着「我们已经不再是持有者了」是**可精确
//!   通知**的，不用去猜自己的窗口 ID
//!
//! ## Wayland
//!
//! 刻意不暴露剪贴板读取权限，没有标准 API。
//! docs/05 的要求是降级为「只读不监听」并明确提示用户改用
//! X11 会话，而不是静默不工作。

use crate::clipboard::MAX_BYTES;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ClientMessageData, ClientMessageEvent, ConnectionExt as XProtoExt, EventMask,
    Window,
};
use x11rb::rust_connection::RustConnection;
use x11rb::CURRENT_TIME as CurrentTime;

/// 轮询间隔。与 macOS 共用一个值，这样回环窗口的推导
/// （`clipboard::POLL_INTERVAL_MS * 3`）对两个平台都成立
const POLL: Duration = crate::clipboard::POLL;

/// 这台机器能不能监听。不能的话返回给用户看的原因
pub fn monitor_blocker() -> Option<String> {
    // Wayland 优先于 DISPLAY 的检查：wlroots 下两个环境变量可能都在，
    // 但只有 Wayland 那套是真在生效的
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        return Some(
            "Wayland 会话不支持自动监听（协议刻意不暴露剪贴板读取权限）。\
             请在登录界面右上角的齿轮里改选「Cinnamon on Xorg」后重新登录"
                .into(),
        );
    }
    if std::env::var("DISPLAY").is_err() {
        return Some("没有 DISPLAY 环境变量，不在 X11 会话里".into());
    }
    None
}

/// X11 连接。单独成函数是因为每个需要它的动作都该独立失败 ——
/// 一次连接失败不该让整个监听线程退出
fn connect() -> Option<(RustConnection, usize)> {
    x11rb::rust_connection::RustConnection::connect(None)
        .map_err(|e| eprintln!("连不上 X 服务器: {e}"))
        .ok()
}

fn clipboard_atom(c: &RustConnection) -> Option<Atom> {
    c.intern_atom(false, b"CLIPBOARD")
        .ok()?
        .reply()
        .ok()
        .map(|r| r.atom)
}

/// 当前持有 CLIPBOARD 的窗口。0 表示无人持有 ——
/// 剪贴板被清空，或上一个持有者已经退出
fn owner_of(c: &RustConnection, sel: Atom) -> Window {
    match c.get_selection_owner(sel) {
        Ok(cookie) => cookie.reply().map(|r| r.owner).unwrap_or(0),
        Err(_) => 0,
    }
}

/// 读当前剪贴板文本。
///
/// 拿不到是正常情况而不是错误：剪贴板可能是空的，也可能放的是
/// 图片或文件列表。监听只关心「有没有新文本」
fn read_text(ctx: &mut arboard::Clipboard) -> Option<String> {
    ctx.get_text().ok()
}

/// 兜底读取的间隔（毫秒）。
///
/// 光靠「owner 变化」会漏掉两种情况，两者都很常见：
///
/// 1. **同一窗口内连续复制**。X11 的 Selection 是按窗口给的，
///    编辑器原地改内容再复制时 owner 窗口没变。macOS 的
///    `changeCount` 没这问题，X11 有 —— 这也是两种机制不能共用
///    一套代码的原因
/// 2. **系统里已经装了别的剪贴板管理器**（GPaste、Clipman 等）。
///    它们会代表所有应用持有 CLIPBOARD，owner 恒定不变，
///    纯 owner 轮询会什么都看不到
///
/// 所以每 2 秒无条件读一次，让内容比对去判断有没有变。
/// 代价是两次 X 往返，比 owner 查询贵一点但可以忽略
const FALLBACK_READ_EVERY: Duration = Duration::from_millis(2000);

/// 造一个「源程序」的剪贴板句柄。
///
/// 只给验收用的 example：需要模拟一个别的应用接管剪贴板、
/// 然后再退出，好验证 DevClip 的接管是否真的保住了内容
pub fn arboard_clipboard() -> Result<arboard::Clipboard, arboard::Error> {
    arboard::Clipboard::new()
}

/// 接管 CLIPBOARD 并一直应答别人的请求，直到内容被覆盖。
///
/// 返回时说明已经有人复制了更新的内容。这是 docs/05 风险 4 的
/// 直接对策：源程序退出后内容不会消失
///
/// 跑在独立线程上且只阻塞在 `wait()` 里 —— 那个调用会一直等到
/// 被覆盖为止，正好就是我们要的等待方式
fn take_ownership(content: String) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("devclip-x11-owner".into())
        .spawn(move || {
            use arboard::SetExtLinux;
            let mut cb = match arboard::Clipboard::new() {
                Ok(cb) => cb,
                Err(e) => {
                    eprintln!("接管剪贴板失败，内容会在源程序退出后丢失: {e}");
                    return;
                }
            };
            if let Err(e) = cb.set().wait().text(content) {
                eprintln!("接管剪贴板失败: {e}");
            }
        })
        .expect("起剪贴板接管线程失败")
}

/// 起监听线程。
///
/// `on_text` 在「这是用户的新复制、且应该入库」时被调用一次。
/// 返回的 `Arc<AtomicBool>` 可用来停掉线程
pub fn spawn_watcher<F>(on_text: F) -> std::io::Result<Arc<AtomicBool>>
where
    F: Fn(String) + Send + 'static,
{
    let running = Arc::new(AtomicBool::new(true));
    let flag = Arc::clone(&running);

    std::thread::Builder::new()
        .name("devclip-clipboard".into())
        .spawn(move || {
            let Some((c, _screen)) = connect() else {
                eprintln!("剪贴板监听未启动：连不上 X 服务器");
                return;
            };
            let Some(sel) = clipboard_atom(&c) else {
                eprintln!("剪贴板监听未启动：拿不到 CLIPBOARD 原子");
                return;
            };

            // 长期持有一个 Clipboard。arboard 内部是进程级单例，
            // 反复 new/drop 只会让引用计数来回抖，而我们要的就是
            // 「常驻的剪贴板管理器」这个身份
            let mut ctx = match arboard::Clipboard::new() {
                Ok(cb) => cb,
                Err(e) => {
                    eprintln!("剪贴板监听未启动：{e}");
                    return;
                }
            };

            // 启动时的基准。不读内容 —— 此刻剪贴板里是用户上次
            // 复制的东西，不该因为程序启动就塞进历史；
            // 密码管理器塞进去的东西也不该在启动瞬间被存下来
            let mut last_owner = owner_of(&c, sel);
            let seen = crate::clipboard::Seen::default();
            if let Some(t) = read_text(&mut ctx) {
                seen.mark_as_seen(&t);
            }

            // 我们自己接管期间的接管线程。置位期间不判断 ——
            // 此时的 owner 是我们，读回来的也是刚存进去的那条，
            // 判它「新内容」就会平白多一条。
            //
            // 接管线程被覆盖后会自动结束，`is_finished` 就是
            // 「我们已把 ownership 让出去」的精确通知，
            // 不用去猜自己的窗口 ID
            let mut our_thread: Option<std::thread::JoinHandle<()>> = None;
            let mut ticks: u32 = 0;

            while flag.load(Ordering::SeqCst) {
                std::thread::sleep(POLL);
                ticks = ticks.wrapping_add(1);

                if our_thread.as_ref().is_some_and(|t| t.is_finished()) {
                    our_thread = None;
                }
                let we_own = our_thread.is_some();

                let owner_changed = !we_own && owner_of(&c, sel) != last_owner;
                let fallback = ticks * POLL >= FALLBACK_READ_EVERY;
                if !owner_changed && !fallback {
                    continue;
                }

                let Some(text) = read_text(&mut ctx) else {
                    // 剪贴板被清空或换成了图片/文件。丢掉基准，
                    // 这样下一次放回同样的文本会被当成新内容
                    last_owner = 0;
                    continue;
                };
                let text = text.trim().to_string();
                if text.is_empty() || text.len() > MAX_BYTES {
                    continue;
                }
                // 去重。我们自己持有时读回来的也是刚写进去的那条，
                // 靠这一步挡住「接管 → 读回 → 存 → 再接管」的循环
                if seen.is_repeat(&text, Instant::now()) {
                    continue;
                }
                if let Some(why) = crate::sensitive::scan(&text) {
                    eprintln!("剪贴板内容疑似敏感（{why}），已跳过入库");
                    continue;
                }

                last_owner = owner_of(&c, sel);
                // 先通知再接管。顺序反了的话，接管后立刻返回的
                // 线程会让下一轮跳过，而这时候内容还没存过
                on_text(text.clone());
                our_thread = Some(take_ownership(text));
            }
        })?;
    Ok(running)
}

/// 当前活动窗口的 X11 窗口 ID。
///
/// 返回 ID 而不是窗口名，这是与 macOS 版对齐的关键：
/// macOS 拿的是 bundle id（稳定、唯一），X11 上对应的东西就是
/// 窗口 ID。早先这里返回窗口标题，然后靠标题找回窗口 ——
/// 而标题会变、可能重复，调色板自己抢到焦点时记下的还是
/// 「DevClip」，回去根本匹配不上
pub fn frontmost_window() -> Option<Window> {
    let (c, screen) = connect()?;
    active_window(&c, c.setup().roots[screen].root)
}

/// 读 root 上的 `_NET_ACTIVE_WINDOW`。
///
/// 属性是 32 位卡片（format=32），x11rb 给的是**小端**字节。
/// 写成 from_be_bytes 会得到 0x600e003 这样的错窗口号 ——
/// 症状是 source_app 永远为空，而且不报错，很难查
fn active_window(c: &RustConnection, root: Window) -> Option<Window> {
    let active = intern(c, "_NET_ACTIVE_WINDOW")?;
    let window_atom: Atom = AtomEnum::WINDOW.into();
    let cookie = c
        .get_property(false, root, active, window_atom, 0, 1)
        .ok()?;
    let value = cookie.reply().ok()?.value;
    let b = value.get(0..4)?;
    let win = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    (win != 0).then_some(win)
}

/// 当前前台应用的窗口名。
///
/// 与 [frontmost_window] 配对使用：ID 用来精确定位回去，
/// 名字只作为给人看的 `source_app`。macOS 那边返回的是
/// bundle id，X11 上没有等价物，窗口名是能拿到的最接近的东西
pub fn frontmost_app() -> Option<String> {
    let (c, _) = connect()?;
    let win = frontmost_window()?;
    window_title(&c, win)
}

fn window_title(c: &RustConnection, win: Window) -> Option<String> {
    let utf8 = intern(c, "UTF8_STRING")?;
    for name_atom in [
        intern(c, "_NET_WM_NAME")?,
        intern(c, "WM_NAME").unwrap_or(0),
    ] {
        if name_atom == 0 {
            continue;
        }
        let Ok(cookie) = c.get_property(false, win, name_atom, utf8, 0, 256) else {
            continue;
        };
        let Ok(r) = cookie.reply() else { continue };
        if r.value.is_empty() {
            continue;
        }
        // 收集字节再按 UTF-8 解码。逐字节 `as char` 会把中文变成
        // 「é¡¹ç»®」这样的乱码 —— 窗口名大多非 ASCII，所以这个 bug
        // 一眼能看见，但也正因如此容易误以为是终端字体问题
        let s = String::from_utf8_lossy(&r.value)
            .trim_matches('\0')
            .to_string();
        if !s.is_empty() {
            return Some(s);
        }
    }
    None
}

/// 把指定窗口拉到前台。
///
/// 走 ICCCM 的 `_NET_ACTIVE_WINDOW` 客户端消息，这是窗口管理器
/// 认可的方式；单发 `XRaiseWindow` 在多数 WM 下会被忽略。
///
/// `data[0] = 2` 是「由用户操作发起」的源码指示，不带它的话
/// 不少 WM 会当成程序自顾自的请求而直接忽略
pub fn activate_window(win: Window) -> Result<(), String> {
    let (c, screen) = connect().ok_or_else(|| "连不上 X 服务器".to_string())?;
    let root = c.setup().roots[screen].root;

    // 窗口可能已经关了（用户复制完就关了那个标签页）
    let reply = c
        .get_window_attributes(win)
        .map_err(|e| format!("目标窗口已失效（0x{win:x}）：{e}"))?
        .reply()
        .map_err(|e| format!("目标窗口已失效（0x{win:x}）：{e}"))?;
    if reply.map_state != x11rb::protocol::xproto::MapState::VIEWABLE {
        return Err(format!("目标窗口已不可见（0x{win:x}）"));
    }

    let active_atom = intern(&c, "_NET_ACTIVE_WINDOW").ok_or("拿不到 _NET_ACTIVE_WINDOW 原子")?;
    let data = ClientMessageData::from([2, CurrentTime, 0, 0, 0]);
    let ev = ClientMessageEvent::new(32, root, active_atom, data);
    c.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_NOTIFY | EventMask::PROPERTY_CHANGE,
        ev,
    )
    .map_err(|e| format!("发送激活事件失败: {e}"))?
    .check()
    .map_err(|e| format!("发送激活事件失败: {e}"))?;
    c.flush().map_err(|e| format!("发送激活事件失败: {e}"))?;
    Ok(())
}

fn intern(c: &RustConnection, name: &str) -> Option<Atom> {
    c.intern_atom(false, name.as_bytes())
        .ok()?
        .reply()
        .ok()
        .map(|r| r.atom)
}

// ── 模拟粘贴（XTEST）─────────────────────────────────────────────────

/// 发一组按键。
///
/// 顺序固定为 **按下修饰键 → 按下主键 → 松开主键 → 松开修饰键**。
/// 少按一个修饰键就是在往目标窗口里打普通字母；顺序错了则
/// 变成按下两个键再同时松开，多数应用不认
pub fn send_paste_keystroke() -> Result<(), String> {
    let (c, _) = connect().ok_or_else(|| "连不上 X 服务器".to_string())?;

    // 现问键盘映射再决定键位，而不是写死码值。键盘布局不同时
    // 硬编码的键位会打到别的键上 —— 比如 Dvorak 布局下
    // 55 号键不是 'v'
    let (ctrl, shift, v) = keycodes(&c)?;

    let send = |keycode: u8, press: bool| -> Result<(), String> {
        use x11rb::protocol::xtest::ConnectionExt as _;
        // 参数是 (type, detail, time, root, rootX, rootY, deviceid)。
        // XTEST 的 FakeInput 不看 root 坐标，但签名上仍要填
        const KEY_PRESS: u8 = 2;
        const KEY_RELEASE: u8 = 3;
        c.xtest_fake_input(
            if press { KEY_PRESS } else { KEY_RELEASE },
            keycode,
            0,
            c.setup().roots[0].root,
            0,
            0,
            0,
        )
        .map_err(|e| format!("XTEST 发送失败: {e}"))?
        .check()
        .map_err(|e| format!("XTEST 发送失败: {e}"))
    };

    send(ctrl, true)?;
    send(shift, true)?;
    send(v, true)?;
    send(v, false)?;
    send(shift, false)?;
    send(ctrl, false)?;
    Ok(())
}

/// 找出 Control_L / Shift_L / 「v」的键位码
fn keycodes(c: &RustConnection) -> Result<(u8, u8, u8), String> {
    // 服务端不提供 min/max keycode 查询（那是 xkb 扩展的事），
    // 直接按 X 协议规定的有键范围 8..=255 查一遍。
    // 多查几十个空键位没有代价，省掉一个扩展依赖
    const MIN_KEYCODE: u8 = 8;
    const COUNT: u8 = 248;
    let map = c
        .get_keyboard_mapping(MIN_KEYCODE, COUNT)
        .map_err(|e| format!("读键盘映射失败: {e}"))?
        .reply()
        .map_err(|e| format!("读键盘映射失败: {e}"))?;

    // XKB_KEY_Control_L=0xFFE3, Shift_L=0xFFE1, 'v'=0x076
    //
    // `keysyms` 是扁平的：每个键位连续 keysyms_per_keycode 个，
    // 所以不能直接在整个数组里搜 —— 搜到的是「下标」，
    // 换算成键位还得除以每键位数
    let per_key = map.keysyms_per_keycode.max(1) as usize;
    let find = |sym: u32| -> Option<u8> {
        map.keysyms
            .chunks(per_key)
            .enumerate()
            .find(|(_, group)| group.contains(&sym))
            .map(|(i, _)| MIN_KEYCODE + i as u8)
    };
    let ctrl = find(0xFFE3).ok_or("当前键盘布局里找不到 Control 键")?;
    let shift = find(0xFFE1).ok_or("当前键盘布局里找不到 Shift 键")?;
    let v = find(0x076).ok_or("当前键盘布局里找不到字母 v")?;
    Ok((ctrl, shift, v))
}

/// 写剪贴板并保持持有
pub fn write_text(text: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut cb| cb.set_text(text.to_owned()))
        .map_err(|e| format!("写入剪贴板失败：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 有 DISPLAY 却没有 Wayland 时才该能监听。这条在 CI 的
    /// Linux runner 上没有 DISPLAY，所以只看「不该误判」
    #[test]
    fn wayland_blocks_monitoring() {
        // 不能改环境变量（并发测试会互相干扰），所以只验证
        // 无 DISPLAY 时一定给得出原因
        if std::env::var("DISPLAY").is_err() {
            assert!(monitor_blocker().is_some(), "没有 DISPLAY 必须给出原因");
        }
    }

    /// 暂停键位探测需要真连接，没有 DISPLAY 时跳过而不是假装通过
    #[test]
    fn finds_keycodes_on_a_live_display() {
        let Some((c, _)) = connect() else {
            eprintln!("没有 X 连接，跳过");
            return;
        };
        let (ctrl, shift, v) = keycodes(&c).expect("标准布局下这三个键位必然存在");
        assert_ne!(ctrl, 0);
        assert_ne!(shift, 0);
        assert_ne!(v, 0);
        // 三个键位不该相同 —— 相同就说明映射读错了
        assert_ne!(ctrl, shift);
        assert_ne!(ctrl, v);
        assert_ne!(shift, v);
    }

    /// 防回归：窗口属性是 32 位卡片，x11rb 返回小端字节。
    /// 之前写成 from_be_bytes，`_NET_ACTIVE_WINDOW` 解析成 0x600e003，
    /// 于是 source_app 永远是空的 —— 而且不报任何错
    #[test]
    fn active_window_is_little_endian() {
        let Some((c, screen)) = connect() else {
            eprintln!("没有 X 连接，跳过");
            return;
        };
        let root = c.setup().roots[screen].root;
        let active = intern(&c, "_NET_ACTIVE_WINDOW").unwrap_or(0);
        let window_atom: Atom = AtomEnum::WINDOW.into();
        let Ok(cookie) = c.get_property(false, root, active, window_atom, 0, 1) else {
            return;
        };
        let Ok(r) = cookie.reply() else { return };
        let Some(b) = r.value.get(0..4) else { return };
        assert_eq!(r.format, 32, "CARD32 属性的 format 应该是 32");
        let win = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        // 小端解析出的窗口号，其四个字节应原样对应原始值
        assert_eq!(
            win.to_le_bytes(),
            [b[0], b[1], b[2], b[3]],
            "字节序解析不自洽"
        );
        // 而大端解析只在字节全相同时才巧合相等，所以拿真实窗口号验证：
        // 它的首字节应该等于属性的末字节
        if win != 0 {
            println!(
                "活动窗口 = 0x{win:x}（大端会误读成 0x{:x}）",
                u32::from_be_bytes([b[0], b[1], b[2], b[3]])
            );
        }
    }

    #[test]
    fn owner_query_does_not_panic_without_display() {
        // 连不上时返回 0（无人持有）而不是 panic —— 监听线程
        // 崩了整个功能就没了
        if let Some((c, _)) = connect() {
            let sel = clipboard_atom(&c).unwrap_or(0);
            let _ = owner_of(&c, sel);
        }
    }
}
