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

/// Linux 实现。与本文件里的 macOS 路径完全独立 ——
/// 两者机制不同（Selection vs changeCount），硬合在一起只会互相拖累
#[cfg(all(unix, not(target_os = "macos")))]
pub mod linux;

use crate::repo;

/// 轮询间隔。再密就是白烧 CPU，每次过一遍 NSPasteboard 不便宜
const POLL: Duration = Duration::from_millis(500);

/// 单条入库的字节上限。往剪贴板里丢几十 MB 日志很常见，
/// 全量入库会让 SQLite 迅速膨胀，而且这种内容在调色板里也没法看
const MAX_BYTES: usize = 1024 * 1024;

/// 敏感内容的存活时间（docs/03：now + 60s）。
/// 到期由后台任务删除并清空剪贴板；再次复制会顺延。
/// 是否启用由设置项 sensitiveAutoExpire 控制，
/// 间隔本身不是设置项 —— 密钥在历史里多躺一分钟都算久
const SENSITIVE_TTL_MS: i64 = 60_000;

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

/// 清空剪贴板。敏感条目到期删除时，如果剪贴板里躺的还是它，
/// 必须一并清掉 —— 条目从历史里消失了，密钥却还在剪贴板上
/// 等着下一个粘贴目标，等于没删
pub fn clear_text() -> Result<(), String> {
    Clipboard::new()
        .and_then(|mut cb| cb.clear())
        .map_err(|e| format!("清空剪贴板失败：{e}"))
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

/// 记住「上一次看到的内容」，用来挡重复。
///
/// ## 为什么不能只比内容
///
/// 早先这里只是个 `Option<String>`，条件是 `text == last`。
/// 看起来能挡住「按住 Ctrl+C 不放」刷屏，但 `last` 只在剪贴板
/// 被清空时才重置 —— 于是「连续两次复制同一段内容」时第二次
/// 会被误判成重复，`copy_count` 永远停在 1。而「从历史里复制
/// 一条命令、改一改、再复制一次」恰恰是常见操作。
///
/// 早先的修法是「内容相同**且**在时间窗口内」才算重复
/// （窗口比 [POLL] 宽，800ms）。这版在真机上被推翻：轮询一圈
/// 不止 `POLL` 那么快 —— `read_text()` 与入库都要花时间，实测
/// 周期能到 1 秒以上，窗口再宽也只是纸面成立。后果不是「多记
/// 几条」，而是每轮都判成新内容，`copy_count` 被刷高，而
/// `expires_at` 被一次次顺延，**敏感项永远不过期**。
///
/// 所以只比内容：剪贴板里没换过东西就是没有新的复制事件。
/// 「隔一分钟又复制了同一段」这种确实区分不出来，代价只是
/// `copy_count` 少加一次（那条记录本来就在历史里）；而中间
/// 复制过别的东西再复制回来（先 a 后 b 再 a）照样算两次。
#[derive(Default)]
pub struct Seen(Mutex<Option<String>>);

impl Seen {
    /// 这段内容和上一次观察到的一样吗
    ///
    /// 判为重复时**也要更新记录** —— 判据是「与上次观察相比
    /// 有没有变」，而不是「与上次入库相比」
    pub fn is_repeat(&self, text: &str) -> bool {
        let mut g = lock(&self.0);
        let repeat = g.as_deref() == Some(text);
        *g = Some(text.to_string());
        repeat
    }

    /// 直接记为已见过，不做重复判定。启动时用来建立基准
    pub fn mark_as_seen(&self, text: &str) {
        *lock(&self.0) = Some(text.to_string());
    }

    /// 剪贴板被清空或换成了图片。丢掉记录，
    /// 这样下一次放回同样的文本会被当成新内容
    pub fn clear(&self) {
        *lock(&self.0) = None;
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
    #[cfg(all(unix, not(target_os = "macos")))]
    return linux::frontmost_app();
    #[cfg(target_os = "windows")]
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
pub fn activate(target: &str) -> Result<(), String> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // 传进来的是十六进制窗口 ID（0x…）。早先这里传窗口名，
        // 靠标题匹配找回窗口 —— 标题会变、可能重复，粘贴经常落空
        let id = u32::from_str_radix(target.trim_start_matches("0x"), 16)
            .map_err(|_| format!("目标窗口标识无法解析：{target}"))?;
        linux::activate_window(id)
    }
    #[cfg(target_os = "windows")]
    {
        Err(format!("当前平台尚未实现唤起目标应用（{target}）"))
    }
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
    #[cfg(all(unix, not(target_os = "macos")))]
    return linux::send_paste_keystroke();
    #[cfg(target_os = "windows")]
    Err("当前平台尚未实现模拟按键".into())
}

/// 起监听线程。平台在这里分派。
///
/// 线程 panic 不影响主流程，但那样就再也收不到新内容了，
/// 所以这里只对「起不来」报错
pub fn spawn_watcher(app: tauri::AppHandle, self_write: Arc<SelfWrite>) {
    #[cfg(all(unix, not(target_os = "macos")))]
    return spawn_linux(app, self_write);

    #[cfg(not(all(unix, not(target_os = "macos"))))]
    spawn_macos(app, self_write)
}

/// macOS 的监听循环：`changeCount` 轮询
#[cfg(not(all(unix, not(target_os = "macos"))))]
fn spawn_macos(app: tauri::AppHandle, self_write: Arc<SelfWrite>) {
    std::thread::Builder::new()
        .name("devclip-clipboard".into())
        .spawn(move || {
            // 先把启动那一刻剪贴板里的东西记成「已见过」：
            // 应用没运行期间复制的东西不该被追溯入库，
            // 密码管理器塞进去的东西也不该在启动瞬间被存下来
            let seen = Seen::default();
            if let Some(t) = read_text() {
                seen.mark_as_seen(&t);
            }
            loop {
                std::thread::sleep(POLL);
                let Some(text) = read_text() else {
                    seen.clear();
                    continue;
                };
                if seen.is_repeat(&text) {
                    continue;
                }
                if self_write.take(&text) {
                    continue;
                }
                if text.trim().is_empty() || text.len() > MAX_BYTES {
                    continue;
                }
                // 敏感内容打标记入库，不再直接跳过。
                //
                // 早先这里是「不入库」，理由是历史明文落盘 —— 但那让
                // 用户毫无感知：复制了密钥，打开调色板什么都没有，
                // 只会以为 DevClip 坏了。按 docs/03 的设计改为入库打标：
                // UI 打锁、默认搜不到（repo::list 排除）、60 秒后由
                // 后台任务删除。暴露窗口从「永久」收敛到 60 秒，
                // 而密钥本来就已经在系统剪贴板里躺着了
                let sensitive = crate::sensitive::scan(&text).is_some();
                if let Some(item) = capture(&app, &text, sensitive) {
                    let _ = tauri::Emitter::emit(&app, "clipboard://changed", item);
                }
            }
        })
        .expect("起剪贴板监听线程失败");
}

/// 入库一条新内容。返回它，供前端增量刷新。
///
/// `sensitive` 由调用方扫好传进来（两个平台的监听路径共用这里，
/// 扫描点必须在入库之前）；到期时间与清理参数在这里按设置快照定
fn capture(app: &tauri::AppHandle, text: &str, sensitive: bool) -> Option<repo::ClipboardItem> {
    // 取前台应用必须在锁外。系统调用一旦变慢（权限弹窗、进程起不来），
    // 就会把整把数据库锁一起拖住，界面和别的命令全卡死
    // 前台应用是自己就记 null。调色板显示时 DevClip 是前台窗口，
    // 记下来会让每条历史都写着「DevClip」，一个有用的字段就废了
    let source_app = frontmost_app().filter(|a| a != &app.config().identifier.to_string());
    // 设置快照也是「取完再碰数据库锁」：Mutex 上不做任何慢操作
    let (max_items, retention_ms, expires_at) = {
        let rt = app.state::<crate::RuntimeSettings>();
        let s = crate::unpoison(&rt.0);
        let ea = if sensitive && s.sensitive_auto_expire {
            Some(repo::now_ms() + SENSITIVE_TTL_MS)
        } else {
            None
        };
        (s.max_items, s.retention_days * 86_400_000, ea)
    };
    let state = app.state::<crate::Db>();
    let conn = lock(&state.conn);
    let (id, created) = repo::upsert(
        &conn,
        &repo::NewItem {
            content: text.to_string(),
            source_app,
            image_path: None,
            sensitive,
            expires_at,
        },
    )
    .ok()?;

    if created {
        // 只在真的新增时清理。重复内容只涨计数，
        // 每次都跑一遍 DELETE 是白费
        let _ = repo::prune(&conn, max_items, Some(retention_ms));
    }
    repo::get(&conn, id).ok().flatten()
}

/// 手动粘贴该按哪个键。两个平台的约定不一样：macOS 是 ⌘V，
/// Linux 桌面环境普遍是 Ctrl+Shift+V —— 单按 ^V 在 GNOME Terminal
/// 里是「显示光标位置」而不是粘贴，Windows 上则会和很多软件的
/// 快捷键打架
pub const PASTE_KEY_HINT: &str = if cfg!(target_os = "macos") {
    "⌘V"
} else {
    "Ctrl+Shift+V"
};

// ── Linux ───────────────────────────────────────────────────────────

/// Linux 的监听：轮询 CLIPBOARD 持有者，变化了就读、入库、并接管。
///
/// 接管不是可选的 —— X11 剪贴板是借用机制（docs/05 风险 4），
/// 源程序一退出内容就没了。剪贴板管理器必须自己变成持有者，
/// 这也是它必须常驻托盘的原因
#[cfg(all(unix, not(target_os = "macos")))]
fn spawn_linux(app: tauri::AppHandle, self_write: Arc<SelfWrite>) {
    if let Some(why) = linux::monitor_blocker() {
        // 明确告诉用户为什么，不能静默不工作
        eprintln!("剪贴板监听未启用：{why}");
        crate::emit_monitor_unavailable(&app, why);
        return;
    }

    let handle = app.clone();
    let sw = Arc::clone(&self_write);
    let res = linux::spawn_watcher(move |text: String| {
        // 回环判断要在入库之前。Linux 上这一步比 macOS 更要紧：
        // 接管线程把自己写的内容又读回来是常态，
        // 判断错位置就会每次接管都多一条历史
        if sw.take(&text) {
            return;
        }
        // 敏感扫描与 macOS 同一套规则。早先 Linux 路径漏了这一步，
        // 密钥在 Linux 上是直接入库的 —— 那正是这个功能最该管住的场景
        let sensitive = crate::sensitive::scan(&text).is_some();
        if let Some(item) = capture(&handle, &text, sensitive) {
            let _ = tauri::Emitter::emit(&handle, "clipboard://changed", item);
        }
    });

    if let Err(e) = res {
        eprintln!("起剪贴板监听线程失败: {e}");
    }
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

    /// 中间复制过别的东西，再复制回来 —— 是两次真实的复制
    #[test]
    fn same_text_after_intervening_content_is_new() {
        let s = Seen::default();
        assert!(!s.is_repeat("a"));
        assert!(!s.is_repeat("b"));
        // 回到 a：剪贴板确实换过内容，这是新的一次复制
        assert!(!s.is_repeat("a"));
    }

    /// 按住 Ctrl+C 不放：每隔一个轮询周期就看到一次内容没变。
    /// 挡不住的话 copy_count 会被刷到几十
    #[test]
    fn held_down_key_does_not_inflate_count() {
        let s = Seen::default();
        assert!(!s.is_repeat("x"));
        for n in 1..=20 {
            assert!(s.is_repeat("x"), "第 {n} 轮不该被当成新内容");
        }
    }

    /// 防回归（真机实测）：轮询一圈不止 POLL 那么快，`read_text()`
    /// 与入库都要花时间，实测周期能到 1 秒以上。早先的「时间窗口」
    /// 去重在这个周期下每轮都判成新内容，`expires_at` 被一次次
    /// 顺延，敏感项永远不过期。这里把观察间隔拉到 1.5 秒 ——
    /// 比任何曾经的窗口都宽 —— 仍必须判为重复
    #[test]
    fn slow_poll_loop_never_recaptures_unchanged_content() {
        let s = Seen::default();
        assert!(!s.is_repeat("sk_live_xxx"), "第一次是入库");
        // 60 秒 TTL 期内大约 40 轮，够覆盖真机上观察到的现象
        for n in 1..40 {
            assert!(
                s.is_repeat("sk_live_xxx"),
                "第 {n} 轮（真机实测约 1s 一轮）被当成了新复制，\
                 expires_at 会被顺延到永不过期"
            );
        }
    }

    /// 换内容时立刻生效
    #[test]
    fn different_text_is_never_a_repeat() {
        let s = Seen::default();
        assert!(!s.is_repeat("a"));
        assert!(!s.is_repeat("b"));
        assert!(!s.is_repeat("c"));
    }

    /// 剪贴板被清空后，放回同样的文本算新内容
    #[test]
    fn clear_makes_the_same_text_new_again() {
        let s = Seen::default();
        assert!(!s.is_repeat("a"));
        assert!(s.is_repeat("a"));
        s.clear();
        assert!(!s.is_repeat("a"));
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
