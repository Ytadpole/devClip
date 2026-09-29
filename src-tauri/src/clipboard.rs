//! 剪贴板读写与监听 —— 阶段 5
//!
//! macOS 上读剪贴板不需要任何授权（TCC 只拦辅助功能与 Apple Events），
//! 所以直接用 arboard 读写，不引 Tauri 插件 —— 前端权限表也不用加东西。
//!
//! 监听用轮询而不是 NSPasteboard 的变更通知：挂通知要在主 run loop 上
//! 装 observer，还得把回调绕回主线程，而我们已经有独立后台线程了。
//! 人复制东西是秒级节奏，500ms 一次足够。

use arboard::Clipboard;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Manager;

use crate::repo;

/// 轮询间隔。再密就是白烧 CPU，每次过一遍 NSPasteboard 不便宜
const POLL: Duration = Duration::from_millis(500);

/// 单条入库的字节上限。往剪贴板里丢几十 MB 日志很常见，
/// 全量入库会让 SQLite 迅速膨胀，而且这种内容在调色板里也没法看
const MAX_BYTES: usize = 1024 * 1024;

/// 保留条数与天数。阶段 7 接到设置项后从 get_settings 取
const MAX_ITEMS: i64 = 1000;
const RETENTION_DAYS: i64 = 30;

/// 锁中毒在别处（lib.rs 的数据库连接）按「报告但别崩」处理，这里同理。
/// 剪贴板状态只是缓存，坏掉重读一次就恢复了，不值得为此杀掉监听线程
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 读剪贴板文本。None 表示当前没有可入库的文本 —— 剪贴板空着，
/// 或者放的是图片、文件列表。那不是错误，只是什么都没发生
pub fn read_text() -> Option<String> {
    let mut cb = Clipboard::new().ok()?;
    // 拿不到文本 flavor 与其他错误在这里没有区别：这一轮就是没有文本。
    // 区分它们没有收益，监听线程只关心「有没有新文本」
    cb.get_text().ok()
}

/// 写剪贴板。错误文案要能直接显示给用户
pub fn write_text(text: &str) -> Result<(), String> {
    Clipboard::new()
        .and_then(|mut cb| cb.set_text(text))
        .map_err(|e| format!("写入剪贴板失败：{e}"))
}

/// 记住「刚才是我们自己写进去的」。
///
/// copy / paste 命令会把内容写回系统剪贴板，监听线程下一轮就会看到它，
/// 再 upsert 一次等于白涨一次 copyCount、多发一次事件给前端。
/// 记一个哈希跳过即可；只记一条，因为写入总是用户一次触发一件事，
/// 且取用即清 —— 之后同样内容从别处复制进来仍会正常入库
#[derive(Default)]
pub struct SelfWrite(Mutex<Option<String>>);

impl SelfWrite {
    pub fn mark(&self, text: &str) {
        *lock(&self.0) = Some(repo::hash(text));
    }

    /// 是我们自己写的就返回 true，并顺手清掉标记
    pub fn take(&self, text: &str) -> bool {
        let mut g = lock(&self.0);
        if g.as_deref() == Some(repo::hash(text).as_str()) {
            *g = None;
            return true;
        }
        false
    }
}

/// 当前前台应用的 bundle id。
///
/// 一开始这里是 osascript 问 System Events，踩了个坑：那会触发 TCC 的
/// 「自动化」授权弹窗。弹窗不出现时 osascript 就一直等，而调用方当时
/// 持着数据库锁 —— 整个应用就这么卡住了（实测复现）。
/// NSWorkspace 是进程内调用，微秒级返回，读前台应用本来就不需要任何授权。
#[cfg(target_os = "macos")]
pub fn frontmost_app() -> Option<String> {
    use objc2_app_kit::NSWorkspace;
    // sharedWorkspace 拿不到说明 NSApplication 没起来，此时没有前台应用
    let ws = NSWorkspace::sharedWorkspace();
    // frontmostApplication 在没有前台应用时返回 nil，那是正常情况不是错误
    let app = ws.frontmostApplication()?;
    app.bundleIdentifier().map(|s| s.to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn frontmost_app() -> Option<String> {
    None
}

/// 把某个 bundle id 拉到前台
#[cfg(target_os = "macos")]
pub fn activate(bundle_id: &str) -> Result<(), String> {
    use objc2_app_kit::{NSApplicationActivationOptions, NSWorkspace};

    // 不在运行的 app 不在这个列表里，所以「找不到」本身就涵盖了未运行的情况。
    // 按 bundle id 逐个比而不是用 runningApplicationsWithBundleIdentifier，
    // 是为了少引一个 objc2-foundation（那只是为了造一个 NSString）
    let ws = NSWorkspace::sharedWorkspace();
    let apps = ws.runningApplications();
    let target = apps.iter().find(|a| {
        a.bundleIdentifier()
            .is_some_and(|b| b.to_string() == bundle_id)
    });
    let Some(target) = target else {
        return Err(format!("目标应用未在运行（{bundle_id}）"));
    };
    // 调色板窗口是 alwaysOnTop，普通 activate 会被压在它下面，
    // 所以必须带上 IgnoringOtherApps 明确要求抢到最前。
    // 这个常量在 macOS 14 上废弃（系统自己决定层级），但 13 及更早仍然有效，
    // 而我们要支持到 13，所以这里明确压掉废弃警告
    #[allow(deprecated)]
    let opts = NSApplicationActivationOptions::ActivateAllWindows
        | NSApplicationActivationOptions::ActivateIgnoringOtherApps;
    if target.activateWithOptions(opts) {
        Ok(())
    } else {
        Err(format!("唤起目标应用失败（{bundle_id}）"))
    }
}

#[cfg(not(target_os = "macos"))]
pub fn activate(_bundle_id: &str) -> Result<(), String> {
    Err("当前平台尚未实现唤起目标应用".into())
}

/// 跑一段 osascript，带硬超时。
///
/// 剩下的模拟按键仍走 osascript：它对「辅助功能未授权」会明确以 -1719
/// 退出，而 CGEvent 那种原生路径在没授权时是静默丢弃的，反而没法给用户提示。
/// 但 osascript 可能卡在 TCC 弹窗上等一个永远不会来的回答，所以必须能超时杀掉
#[cfg(target_os = "macos")]
fn osascript(script: &str) -> Result<std::process::Output, String> {
    const TIMEOUT: Duration = Duration::from_secs(3);

    let mut child = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("调用 osascript 失败：{e}"))?;

    // Command::output() 会一直等子进程退出，这里得自己轮询
    let deadline = std::time::Instant::now() + TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                // 拿输出。子进程已退出，管道会正常读到 EOF 不会卡
                return child
                    .wait_with_output()
                    .map_err(|e| format!("读取 osascript 输出失败：{e}"));
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    // 等不出来就杀掉。留着它只会占着一个僵尸进程
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("调用系统辅助功能超时（3 秒）".into());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(format!("等待 osascript 失败：{e}")),
        }
    }
}

#[cfg(target_os = "macos")]
pub fn send_paste_keystroke() -> Result<(), String> {
    let out =
        osascript("tell application \"System Events\" to keystroke \"v\" using command down")?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    if out.status.code() == Some(1719) || err.contains("assistive access") {
        return Err(
            "缺少「辅助功能」授权：请到 系统设置 → 隐私与安全性 → 辅助功能，勾选 DevClip".into(),
        );
    }
    Err(format!("模拟按键失败：{}", err.trim()))
}

#[cfg(not(target_os = "macos"))]
pub fn send_paste_keystroke() -> Result<(), String> {
    Err("当前平台尚未实现模拟按键".into())
}

/// 起监听线程。线程 panic 不影响主流程，但那样就再也收不到新内容了，
/// 所以这里只对「起不来」报错
pub fn spawn_watcher(app: tauri::AppHandle, self_write: Arc<SelfWrite>) {
    std::thread::Builder::new()
        .name("devclip-clipboard".into())
        .spawn(move || {
            // 先把启动那一刻剪贴板里的东西记成「已见过」：
            // 应用没运行期间复制的东西不该被追溯入库，
            // 密码管理器塞进去的东西也不该在启动瞬间被存下来
            let mut last = read_text();
            loop {
                std::thread::sleep(POLL);
                let Some(text) = read_text() else {
                    // 剪贴板被清空或换成了图片。丢掉 last，
                    // 这样下一次放回同样的文本会被当成新内容
                    last = None;
                    continue;
                };
                if Some(&text) == last.as_ref() {
                    continue;
                }
                let ours = self_write.take(&text);
                last = Some(text.clone());
                if ours {
                    continue;
                }
                if text.trim().is_empty() || text.len() > MAX_BYTES {
                    continue;
                }
                if let Some(item) = capture(&app, &text) {
                    let _ = tauri::Emitter::emit(&app, "clipboard://changed", item);
                }
            }
        })
        .expect("起剪贴板监听线程失败");
}

/// 入库一条新内容。返回它，供前端增量刷新
fn capture(app: &tauri::AppHandle, text: &str) -> Option<repo::ClipboardItem> {
    // 取前台应用必须在锁外。系统调用一旦变慢（权限弹窗、进程起不来），
    // 就会把整把数据库锁一起拖住，界面和别的命令全卡死
    let source_app = frontmost_app();
    let state = app.state::<crate::Db>();
    let conn = lock(&state.conn);
    let (id, created) = repo::upsert(
        &conn,
        &repo::NewItem {
            content: text.to_string(),
            source_app,
            image_path: None,
        },
    )
    .ok()?;

    if created {
        // 只在真的新增时清理。重复内容只涨计数，
        // 每次都跑一遍 DELETE 是白费
        let _ = repo::prune(&conn, MAX_ITEMS, Some(RETENTION_DAYS * 86_400_000));
    }
    repo::get(&conn, id).ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_write_is_skipped_exactly_once() {
        let sw = SelfWrite::default();
        sw.mark("hello");
        // 第一次是我们自己写的，要跳过
        assert!(sw.take("hello"));
        // 标记已消耗，同样的内容再出现就该正常入库
        assert!(!sw.take("hello"));
    }

    #[test]
    fn self_write_does_not_swallow_other_content() {
        let sw = SelfWrite::default();
        sw.mark("hello");
        // 监听期间用户复制了别的东西，这条不能被标记吃掉
        assert!(!sw.take("world"));
        // 属于我们的那条仍然有效
        assert!(sw.take("hello"));
    }

    #[test]
    fn take_without_mark_is_never_ours() {
        let sw = SelfWrite::default();
        assert!(!sw.take("anything"));
    }

    #[test]
    fn mark_replaces_the_previous_one() {
        let sw = SelfWrite::default();
        sw.mark("first");
        sw.mark("second");
        // 只记一条：先来的那条已经过期，不该再被抑制
        assert!(!sw.take("first"));
        assert!(sw.take("second"));
    }

    #[test]
    fn self_write_survives_a_poisoned_lock() {
        let sw = SelfWrite::default();
        sw.mark("hello");
        // 模拟某次 panic 把锁毒化了
        let _ = std::panic::catch_unwind(|| {
            let _g = lock(&sw.0);
            panic!("boom");
        });
        // 毒化之后仍能读写，只是锁中毒不再传播成第二次 panic
        assert!(sw.take("hello"));
    }

    /// 防回归：早先 frontmost_app 走 osascript 问 System Events，
    /// 在没给「自动化」授权时会卡在 TCC 弹窗上永不返回 ——
    /// 调用方持着数据库锁，整个应用随之假死（实测复现）。
    /// NSWorkspace 是进程内调用，必须立刻返回
    #[test]
    #[cfg(target_os = "macos")]
    fn frontmost_app_never_blocks() {
        let start = std::time::Instant::now();
        let _ = frontmost_app();
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "读前台应用阻塞了 {:?}，多半又退回走 osascript 了",
            start.elapsed()
        );
    }

    /// 唤起一个不存在的应用要立刻失败，不能挂着等
    #[test]
    #[cfg(target_os = "macos")]
    fn activate_unknown_bundle_fails_fast() {
        let start = std::time::Instant::now();
        let r = activate("com.devclip.definitely-not-running");
        assert!(r.is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
