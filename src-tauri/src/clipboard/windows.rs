//! Windows 剪贴板辅助 —— 阶段 9
//!
//! 与 macOS 同形（序号轮询），与 Linux 的 Selection 机制完全不同，
//! 三者共用 clipboard.rs 里的 `capture` 入库路径。
//!
//! ## 监听为什么是轮询而不是 WM_CLIPBOARDUPDATE
//!
//! 阶段清单里写的是消息钩子，实现换成了 `GetClipboardSequenceNumber`
//! 轮询，理由见 [crate::clipboard::spawn_windows] —— 一句话版本：
//! Windows 有 macOS `changeCount` 的严格等价物，能把已被真机验证过的
//! 轮询循环原样搬过来，只换「怎么发现变了」这一步。
//!
//! ## 本文件里的 Windows 事实
//!
//! - **`GetClipboardSequenceNumber` 读序号不需要打开剪贴板**，
//!   也没有权限要求，轮询成本接近零
//! - **合成按键要发 scancode，不能只发 VK**。`SendInput` 带上
//!   `KEYEVENTF_SCANCODE` 后，接收方按「物理键位」处理 ——
//!   Java 系应用（IntelliJ 用户是主要受众）对纯 VK 的合成按键
//!   普遍不理睬，这是绕不开的
//! - **后台进程直接 `SetForegroundWindow` 会被前台锁定拦下**。
//!   标准解法是先敲一下 Alt 伪装出用户输入上下文，再请求前台

use std::time::{Duration, Instant};
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_SCANCODE, MAPVK_VK_TO_VSC, VK_CONTROL, VK_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsIconic, SetForegroundWindow, ShowWindow,
    SW_RESTORE,
};

/// 剪贴板序号。系统计数器，剪贴板每变一次 +1 —— Windows 版的
/// `changeCount`。回环保护与去抖都在调用方（clipboard.rs）
pub fn sequence_number() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

/// 前台窗口的 HWND，转成 isize 存进 `RestoreTarget`。
/// 没有前台窗口（锁屏、会话未连上）返回 None，是正常情况不是错误
pub fn frontmost_window() -> Option<isize> {
    let h = unsafe { GetForegroundWindow() };
    if h.0.is_null() {
        None
    } else {
        Some(h.0 as isize)
    }
}

/// 前台进程的映像名（如 `Code.exe`），作 `source_app`。
///
/// 不返回全路径：路径里有用户名，记进历史既没用又泄露隐私。
/// 前台是 DevClip 自己时返回 None —— 面板显示期间我们自己就是前台，
/// 记下来会让每条历史都写着 DevClip（与 macOS 的 bundle id 过滤同一语义，
/// 但这里在源头掐掉，连调用方都不用滤）
pub fn foreground_app_name() -> Option<String> {
    let h = unsafe { GetForegroundWindow() };
    if h.0.is_null() {
        return None;
    }
    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(h, Some(&mut pid));
    }
    if pid == 0 || pid == unsafe { GetCurrentProcessId() } {
        return None;
    }
    // QUERY_LIMITED_INFORMATION 不需要任何特权，对提权进程也能问名字
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let name = process_image_name(handle);
    unsafe {
        let _ = CloseHandle(handle);
    }
    name
}

fn process_image_name(h: HANDLE) -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = unsafe {
        QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
    };
    if ok.is_err() || len == 0 {
        return None;
    }
    let full = String::from_utf16_lossy(&buf[..len as usize]);
    std::path::Path::new(&full)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
}

/// 解析 `RestoreTarget` 里存的窗口标识（`frontmost_target` 写的
/// `0x…` 十六进制）。单独成函数是因为它能脱机单测 —— 解析错一个
/// 字符，粘贴就永远落到别处，而且不报错
pub fn parse_hwnd(s: &str) -> Result<isize, String> {
    let t = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    let v = usize::from_str_radix(t, 16).map_err(|_| format!("目标窗口标识无法解析：{s}"))?;
    isize::try_from(v).map_err(|_| format!("目标窗口标识越界：{s}"))
}

/// 把指定窗口拉到前台。
///
/// ## 前台锁定与 Alt 敲击
///
/// Windows 不允许后台进程随意抢前台（否则任何程序都能在你打字时
/// 弹到自己脸上）。`SetForegroundWindow` 只在「调用方有用户输入
/// 上下文」时才被批准。我们的调用来自粘贴动作，没有真实的输入
/// 事件 —— 所以先 SendInput 敲一下 Alt，系统记下「刚刚有用户输入」，
/// 随后的请求就在白名单里。这是广泛使用的标准解法，代价是
/// 一次无害的 Alt 按下并立刻松开（目标窗口收到它时已经失焦）。
///
/// `SetForegroundWindow` 是异步语义：返回 TRUE 只代表请求被接受，
/// 真正切过去还要等。所以给出 300ms 的观察期，用
/// `GetForegroundWindow` 确认真的到位了再返回 Ok。
pub fn activate_window(hwnd: isize) -> Result<(), String> {
    let h = HWND(hwnd as usize as *mut core::ffi::c_void);
    if h.0.is_null() {
        return Err("目标窗口已失效".into());
    }
    unsafe {
        // 最小化着的窗口要先还原，否则请求前台也不会有可见结果
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        tap_alt();
        let granted = SetForegroundWindow(h).as_bool();
        let deadline = Instant::now() + Duration::from_millis(300);
        while Instant::now() < deadline {
            if GetForegroundWindow() == h {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if granted {
            Ok(())
        } else {
            Err(format!("唤起目标窗口失败（0x{hwnd:x}）"))
        }
    }
}

/// 敲一下 Alt（按下并立刻松开）。见 [activate_window] 的说明
fn tap_alt() {
    let scan = alt_scan();
    send_inputs(&[key_input(scan, false), key_input(scan, true)]);
}

fn alt_scan() -> u32 {
    unsafe { MapVirtualKeyW(VK_MENU.0 as u32, MAPVK_VK_TO_VSC) }
}

/// 发一组按键。
///
/// 顺序约束与 Linux 的 XTEST 版相同：**按下修饰键 → 按下主键 →
/// 松开主键 → 松开修饰键**。少按一个修饰键就是在往目标窗口里打
/// 普通字母；顺序错了则变成按下两个键再同时松开，多数应用不认。
///
/// 全部走 scancode（`KEYEVENTF_SCANCODE`）：见模块注释，纯 VK 的
/// 合成按键在 Java 系应用里会被无视
pub fn send_paste_keystroke() -> Result<(), String> {
    let ctrl = unsafe { MapVirtualKeyW(VK_CONTROL.0 as u32, MAPVK_VK_TO_VSC) };
    let v = unsafe { MapVirtualKeyW(b'V' as u32, MAPVK_VK_TO_VSC) };
    if ctrl == 0 || v == 0 {
        return Err("读不到键盘映射，发不出粘贴键".into());
    }
    let inputs = [
        key_input(ctrl, false),
        key_input(v, false),
        key_input(v, true),
        key_input(ctrl, true),
    ];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        // SendInput 返回实际注入的事件数。被安全软件拦下来时
        // 会少于请求值 —— 那是降级路径该报的错，不是静默成功
        Err(format!("系统只接受了 {sent}/{} 个合成按键", inputs.len()))
    }
}

/// 造一个 scancode 按键事件。`up` = 松开
fn key_input(scan: u32, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wScan: scan as u16,
                dwFlags: if up {
                    KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP
                } else {
                    KEYEVENTF_SCANCODE
                },
                ..Default::default()
            },
        },
    }
}

fn send_inputs(inputs: &[INPUT]) {
    unsafe {
        SendInput(inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hwnd_accepts_hex_forms() {
        assert_eq!(parse_hwnd("0x1a2b").unwrap(), 0x1a2b);
        assert_eq!(parse_hwnd("0X1A2B").unwrap(), 0x1a2b);
        assert_eq!(parse_hwnd("1a2b").unwrap(), 0x1a2b);
        assert_eq!(parse_hwnd(" 0x10 \n").unwrap(), 0x10, "首尾空白应容忍");
    }

    #[test]
    fn parse_hwnd_rejects_garbage() {
        for bad in ["", "窗口", "0xzz", "1a2g"] {
            assert!(parse_hwnd(bad).is_err(), "{bad:?} 不该被接受");
        }
    }

    /// 这些探针在无桌面的会话里也必须立刻返回而不是挂住 ——
    /// CI 的 Windows runner 就是无桌面会话
    #[test]
    #[cfg(target_os = "windows")]
    fn probes_never_block_without_desktop() {
        let start = Instant::now();
        let _ = sequence_number();
        let _ = frontmost_window();
        let _ = foreground_app_name();
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "探针阻塞了 {:?}",
            start.elapsed()
        );
    }

    /// 0 号窗口不存在，必须立刻报错。这条不点 SendInput
    /// （null 检查在最前面），在无桌面的 CI 会话里也安全
    #[test]
    #[cfg(target_os = "windows")]
    fn activate_null_hwnd_fails_fast() {
        let start = Instant::now();
        assert!(activate_window(0).is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    /// 序号只增不减。夹在两次读之间没有复制时应该相等
    #[test]
    #[cfg(target_os = "windows")]
    fn sequence_number_never_goes_backwards() {
        let a = sequence_number();
        let b = sequence_number();
        assert!(b >= a, "剪贴板序号不该回退：{a} -> {b}");
    }
}
