/**
 * Rust 侧 —— 阶段 4：SQLite + FTS5
 *
 * 与 src/lib/api.ts 的 ClipboardApi 一一对应。
 *
 * 分层：
 * - db.rs       连接、PRAGMA、schema、FTS5 虚表与触发器
 * - repo.rs     全部 SQL（upsert 去重、query 过滤、增删改）
 * - clipboard.rs 系统剪贴板读写、轮询监听、macOS 前台应用
 * - 本文件      只做「取连接 → 调 repo → 返回」，不写 SQL
 *
 * 字段名约定：serde rename_all = "camelCase"，
 * 与前端 api.ts 的 camelCase 字段对齐。
 */
// db / repo 设为 pub 是给 examples/seed.rs 用的：
// 它要直接调 upsert 灌数据，绕不过这层
pub mod clipboard;
pub mod db;
mod detect;
mod hotkey;
pub mod repo;
mod sensitive;
mod settings;
pub mod toolbox;

use clipboard::SelfWrite;
use db::DbError;
use repo::{ClipboardItem, Query};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use toolbox::ToolboxAction;

/// 调色板弹出前的前台应用，粘贴时要回到那里。
///
/// 不能用 item.source_app —— 那是内容当初被复制时的来源。
/// 用户多半是在编辑器里按快捷键，却要粘上周在终端里复制的 JSON，
/// 回到来源应用去就错了
#[derive(Default)]
pub struct RestoreTarget(Mutex<Option<String>>);

/// 锁中毒按「报告但别崩」处理，与 Db 里的 conn 一致
fn unpoison<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 注入给所有命令的数据库连接。
///
/// rusqlite::Connection 不是 Send（内部有裸指针），而 Tauri 的
/// managed state 要求 Send + Sync，所以必须用 Mutex 包一层。
/// 命令都是同步的，持锁期间不会跨 await 点，不存在死锁风险
pub struct Db {
    pub conn: std::sync::Mutex<rusqlite::Connection>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub hotkey: String,
    pub max_items: i64,
    pub retention_days: i64,
    pub max_image_bytes: i64,
    pub theme: String,
    pub sensitive_auto_expire: bool,
}

/// ActionResult 序列化为 { ok: true, value } 或 { ok: false, error }
#[derive(Debug, Serialize, Deserialize)]
pub struct ActionResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ActionResult {
    pub fn ok(v: impl Into<String>) -> Self {
        ActionResult {
            ok: true,
            value: Some(v.into()),
            error: None,
        }
    }
    pub fn err(e: impl Into<String>) -> Self {
        ActionResult {
            ok: false,
            value: None,
            error: Some(e.into()),
        }
    }
}

fn default_settings() -> Settings {
    Settings {
        hotkey: "Alt+Shift+V".into(),
        max_items: 1000,
        retention_days: 30,
        max_image_bytes: 10 * 1024 * 1024,
        theme: "dark".into(),
        sensitive_auto_expire: true,
    }
}

// ── Tauri commands ───────────────────────────────────────────────
//
// SQL 全在 repo.rs，这里只做「取连接 → 调 repo → 返回」。

/// 命令执行出错时返回可读信息，而不是让 Tauri 抛裸错误。
/// 前端会把它显示在状态栏，所以措辞要能指导下一步
fn err(context: &str, e: DbError) -> String {
    format!("{context}：{e}")
}

/// 取连接。锁中毒说明某条命令 panic 过，那是真 bug，
/// 但没必要让整个应用崩在这里 —— 报错让用户能继续用其他功能
fn lock<'a>(
    state: &'a State<Db>,
) -> Result<std::sync::MutexGuard<'a, rusqlite::Connection>, String> {
    state
        .conn
        .lock()
        .map_err(|_| "数据库连接已失效，请重启应用".to_string())
}

#[tauri::command]
fn list_items(state: State<Db>, q: Query) -> Result<Vec<ClipboardItem>, String> {
    let c = lock(&state)?;
    repo::list(&c, &q).map_err(|e| err("查询历史失败", e))
}

#[tauri::command]
fn get_item(state: State<Db>, id: i64) -> Result<Option<ClipboardItem>, String> {
    let c = lock(&state)?;
    repo::get(&c, id).map_err(|e| err("读取失败", e))
}

#[tauri::command]
fn toggle_favorite(state: State<Db>, id: i64) -> Result<bool, String> {
    let c = lock(&state)?;
    repo::toggle_favorite(&c, id).map_err(|e| err("切换收藏失败", e))
}

#[tauri::command]
fn remove_items(state: State<Db>, ids: Vec<i64>) -> Result<(), String> {
    let c = lock(&state)?;
    repo::remove(&c, &ids).map_err(|e| err("删除失败", e))
}

#[tauri::command]
fn clear_all(state: State<Db>) -> Result<(), String> {
    let c = lock(&state)?;
    repo::clear(&c).map_err(|e| err("清空失败", e))
}

/// 阶段 5 的剪贴板监听要用：入库去重走这里
#[tauri::command]
fn add_item(
    state: State<Db>,
    content: String,
    source_app: Option<String>,
) -> Result<ClipboardItem, String> {
    let c = lock(&state)?;
    let (id, _) = repo::upsert(
        &c,
        &repo::NewItem {
            content,
            source_app,
            image_path: None,
        },
    )
    .map_err(|e| err("保存失败", e))?;
    repo::get(&c, id)
        .map_err(|e| err("读取失败", e))?
        .ok_or_else(|| "保存后读不到该条".to_string())
}

#[tauri::command]
fn item_count(state: State<Db>) -> Result<i64, String> {
    let c = lock(&state)?;
    repo::count(&c).map_err(|e| err("统计失败", e))
}

/// 收起调色板。Esc 的第三级 —— 菜单没开、搜索框也是空的时候
///
/// 走自定义命令而不是前端的 getCurrentWindow().hide()：
/// 后者要用到 core:window:allow-hide 权限，等于把窗口控制权整个交给前端；
/// 自定义命令不需要 capability，权限面小一圈
#[tauri::command]
fn hide_window(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

/// 写回系统剪贴板。写入前要在 SelfWrite 记一笔，
/// 否则监听线程下一轮会把它当成用户新复制的内容再入库一次
#[tauri::command]
fn copy_to_clipboard(
    state: State<Db>,
    sw: State<'_, Arc<SelfWrite>>,
    id: i64,
) -> Result<(), String> {
    let content = {
        let c = lock(&state)?;
        repo::get(&c, id)
            .map_err(|e| err("读取失败", e))?
            .ok_or_else(|| "该条已被删除".to_string())?
            .content
    };
    clipboard::write_text(&content)?;
    sw.mark(&content);
    Ok(())
}

/// 粘到弹出调色板前的前台应用。
///
/// 顺序不能换：先写剪贴板，再收起自己的窗口把焦点让出去，
/// 目标应用到前台之后再等它稳定，最后才发 ⌘V。
/// 少任何一步，按键都会落到 DevClip 自己身上
///
/// 必须是 async：同步命令按 Tauri 的默认 ExecutionContext 直接跑在主线程上，
/// 而这里有等待和外部进程调用 —— 放主线程会把整个事件循环冻住
#[tauri::command]
async fn paste(
    app: tauri::AppHandle,
    state: State<'_, Db>,
    sw: State<'_, Arc<SelfWrite>>,
    target: State<'_, RestoreTarget>,
    id: i64,
) -> Result<(), String> {
    let content = {
        let c = lock(&state)?;
        repo::get(&c, id)
            .map_err(|e| err("读取失败", e))?
            .ok_or_else(|| "该条已被删除".to_string())?
            .content
    };

    clipboard::write_text(&content)?;
    sw.mark(&content);

    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }

    let back = unpoison(&target.0).take();
    if let Some(bundle) = back {
        if let Err(e) = clipboard::activate(&bundle) {
            // 窗口已经藏起来了，不还原的话用户只会看到调色板凭空消失，
            // 报错文案根本没人看得见
            show_palette(&app);
            return Err(e);
        }
        // 目标应用激活是异步的，图标弹回动画期间发键会被丢掉
        std::thread::sleep(std::time::Duration::from_millis(120));
    }
    // 降级路径（docs/05 风险 2）：模拟按键失败时**不能让功能死掉**。
    // 内容已经写进剪贴板了，所以至少还能让用户手动按一下 ——
    // 一个「偶尔要手动按 ^V」的版本，远好过一个「粘贴按钮点不动」的版本
    match clipboard::send_paste_keystroke() {
        Ok(()) => Ok(()),
        Err(e) => {
            // 窗口已经藏了、目标也激活了，不弹回来就没有任何提示，
            // 用户只会看到「点了没反应」
            show_palette(&app);
            // emit 而不是直接改 store：命令层拿不到 store，
            // 而前端已经在监听这个事件（api.ts 的 subscribe 同一条通道）
            let _ = tauri::Emitter::emit(
                &app,
                "clipboard://notice",
                serde_json::json!({
                    "text": format!(
                        "已复制到剪贴板，请手动按 {} 粘贴",
                        clipboard::PASTE_KEY_HINT
                    ),
                    "reason": e,
                }),
            );
            Ok(())
        }
    }
}

#[tauri::command]
fn available_actions(content_type: String) -> Vec<ToolboxAction> {
    // 注册表是唯一的事实来源。前端不认任何具体类型 ——
    // 加一类动作只改 toolbox/mod.rs 一处
    let t = content_type.parse().unwrap_or(detect::ContentType::Text);
    toolbox::for_type(t)
}

/// 跑一个工具箱动作。返回的是**一句摘要**，不是结果本身。
///
/// 结果已经写进系统剪贴板了：工具箱的用处就是「变换完直接粘」，
/// 而调色板状态栏只有一行、2.6 秒后自动消失 —— 塞不下格式化后的 JSON。
/// 写剪贴板前必须在 `SelfWrite` 记一笔，否则监听线程会把这个
/// 结果当成用户在别处复制的内容，又存一条
#[tauri::command]
fn run_toolbox_action(
    db: State<'_, Db>,
    sw: State<'_, Arc<SelfWrite>>,
    id: i64,
    action_id: String,
) -> ActionResult {
    let entry = match toolbox::find(&action_id) {
        Some(e) => e,
        None => return ActionResult::err(format!("没有这个动作：{action_id}")),
    };
    let item = {
        let conn = unpoison(&db.conn);
        match repo::get(&conn, id) {
            Ok(Some(it)) => it,
            Ok(None) => return ActionResult::err(format!("第 {id} 条已不在历史里")),
            Err(e) => return ActionResult::err(format!("读取失败：{e}")),
        }
    };
    // 动作是按 id 找到的，但还得确认它对**这一条**适用。
    // UI 只会给出适用的动作，所以这条防线平时不触发；它是给
    // 「invoke 传了别的类型的动作」兜底的 —— 否则一句普通文本
    // 会被当 JSON 解析，报一个跟用户操作对不上的错
    let t: detect::ContentType = item
        .content_type
        .parse()
        .unwrap_or(detect::ContentType::Text);
    if !entry.applies_to.contains(&t) {
        return ActionResult::err(format!("「{}」不能用在 {} 上", entry.label, t.as_str()));
    }
    match (entry.run)(&item.content) {
        Ok(value) => {
            sw.mark(&value);
            if let Err(e) = clipboard::write_text(&value) {
                return ActionResult::err(format!("动作成功但写不回剪贴板：{e}"));
            }
            let n = item.content.len();
            ActionResult::ok(format!(
                "{}：{} → {}，已复制到剪贴板",
                entry.label,
                human(n),
                human(value.len())
            ))
        }
        Err(e) => ActionResult::err(e),
    }
}

/// 字节数转成人看得懂的量级。摘要里报体积比报字符数有用 ——
/// 「2.1 KB」比「2104」更容易判断要不要粘
fn human(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{:.1} MB", b / (KB * KB))
    }
}

#[tauri::command]
fn get_settings() -> Settings {
    default_settings()
}

#[tauri::command]
fn set_settings(_patch: serde_json::Value) -> Settings {
    // TODO: 阶段 7 合并 patch 并持久化
    default_settings()
}

// ── 窗口与快捷键 ─────────────────────────────────────────────────

/// 让调色板显形。窗口启动时是隐藏的（见 tauri.conf.json），
/// 全靠快捷键呼出来
fn show_palette(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// 快捷键的显隐切换。记下前台应用：用户多半是从别的应用按快捷键过来的，
/// 粘贴时要回到那里。自己的 bundle id 要排除掉 —— 从 Dock 点开应用后
/// 我们会是前台，此时按快捷键如果把 DevClip 自己记进去，
/// 粘贴时就会激活自己，⌘V 按在调色板上
fn toggle_palette(app: &tauri::AppHandle) {
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    if matches!(w.is_visible(), Ok(true)) {
        let _ = w.hide();
        return;
    }
    // 必须在 show_palette 之前取：调色板一旦显示就会抢到焦点，
    // 那时再问前台窗口，问到的已经是 DevClip 自己
    if let Some(target) = frontmost_target() {
        if !is_own_window(app, &target) {
            *unpoison(&app.state::<RestoreTarget>().0) = Some(target);
        }
    }
    show_palette(app);
}

/// 粘贴前记下前台窗口，粘贴后要回到那里。
///
/// 存的是**窗口 ID**（X11）或 **bundle id**（macOS），不是显示用的
/// 名字。名字会变、可能重复，靠它匹配回去经常落空
pub fn frontmost_target() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        clipboard::frontmost_app()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        clipboard::linux::frontmost_window().map(|w| format!("{w:#x}"))
    }
    #[cfg(target_os = "windows")]
    {
        None
    }
}

/// 这是不是 DevClip 自己的窗口。
///
/// 调色板呼出时会抢到焦点，而 X11 上 `XSetInputFocus` 之后
/// `_NET_ACTIVE_WINDOW` 指向的就是我们自己。不排除的话会出现
/// 两个后果：`source_app` 记成「DevClip」，以及粘贴时试图
/// 唤起 DevClip 自己，⌘V/^V 全打在调色板上
fn is_own_window(app: &tauri::AppHandle, target: &str) -> bool {
    let Some(w) = app.get_webview_window("main") else {
        return false;
    };
    let own: String = w.title().unwrap_or_default().to_string();
    // 窗口标题是判断依据之一，但平台各异；真正稳的是配置里的
    // identifier —— 两个都查，任一命中就算自己
    own == target || target == app.config().identifier
}

/// 告诉前端「这台机器不能监听」，界面据此显示原因而不是
/// 假装在正常工作
pub(crate) fn emit_monitor_unavailable(app: &tauri::AppHandle, reason: String) {
    let _ = tauri::Emitter::emit(
        app,
        "clipboard://unavailable",
        serde_json::json!({ "reason": reason }),
    );
}

/// 设置文件位置，与数据库同目录
fn settings_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    dir.join("settings.json")
}

/// 注册全局快捷键。注册不上就把窗口放出来 ——
/// 多半是被别的应用抢占了或系统没给权限。这时用户如果还进不来，
/// 就只剩杀进程重装，比多弹一个窗口糟糕得多
///
/// 优先用用户上次设过的；没设过才用平台默认值。
/// 早先每次启动都用默认值，用户改了也没用 —— 文档里那一项
/// 写得再清楚也没意义
fn register_hotkey(app: &tauri::AppHandle) {
    let platform = hotkey::Platform::current();
    let stored = settings::load(&settings_path(app));
    let spec = stored
        .hotkey
        .unwrap_or_else(|| hotkey::default_hotkey(platform).to_string());

    let Ok(shortcut) = spec.parse::<Shortcut>() else {
        eprintln!("快捷键 {spec} 无法解析，窗口改为常驻显示");
        return show_palette(app);
    };

    // 冲突预判。系统占用情况只有真注册时才知道，那是上面那步；
    // 这里拦的是**已知**被占用的组合 —— 提前说清楚「被谁占了」
    // 比让用户看着一个注册失败强
    if let Err(why) = hotkey::check(platform, &shortcut) {
        eprintln!("快捷键 {spec} 不建议使用：{why}");
    }

    if let Err(e) = app.global_shortcut().register(shortcut) {
        eprintln!("快捷键 {spec} 注册失败（{e}），窗口改为常驻显示");
        show_palette(app);
    }
}

/// 建托盘图标。
///
/// X11 上这是**可用性前提**，不是装饰。X11 剪贴板是借用机制
/// （docs/05 风险 4）：内容存在持有者进程里，进程一退就没了。
/// 所以 DevClip 必须常驻，而「怎么常驻」只能靠托盘 —— 没有它
/// 用户唯一能做的就是杀进程，那恰好是最容易丢数据的操作。
///
/// macOS 上同理：应用跑在 dock 里，但托盘能给一个明确的退出入口
fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;

    let show = MenuItem::with_id(app, "show", "显示调色板", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 DevClip", true, None::<&str>)?;
    // 菜单里明说退出意味着什么。X11 上用户不知道「退出 = 剪贴板
    // 里没被存下的东西会丢」，而这正是最该说清的一句
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut builder = TrayIconBuilder::with_id("devclip")
        .menu(&menu)
        .tooltip("DevClip — 剪贴板历史")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_palette(app),
            "quit" => app.exit(0),
            _ => {}
        })
        // 左键直接开调色板，比「右键才出菜单」少一次点击
        .on_tray_icon_event(|tray, event| {
            use tauri::tray::TrayIconEvent;
            if let TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = event
            {
                show_palette(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// 改快捷键。返回规范化后的值。
///
/// 前端会直接显示 Err 的内容，所以文案要能照着做 ——
/// 尤其是「已被占用」和「格式不对」必须分开说
#[tauri::command]
fn set_hotkey(app: tauri::AppHandle, accel: String) -> Result<String, String> {
    let platform = hotkey::Platform::current();
    let shortcut: Shortcut = accel
        .parse()
        .map_err(|_| format!("无法识别这个快捷键：{accel}。用 Alt+Shift+V 这种写法"))?;

    hotkey::check(platform, &shortcut)?;

    let gs = app.global_shortcut();
    // 先注销旧的再注册新的。不注销的话平台会报「已被占用」，
    // 而占用者其实是我们自己上一个组合
    let stored = settings::load(&settings_path(&app));
    if let Some(old) = stored.hotkey.as_ref() {
        if let Ok(old_hk) = old.parse::<Shortcut>() {
            let _ = gs.unregister(old_hk);
        }
    }

    gs.register(shortcut)
        .map_err(|e| format!("快捷键注册失败：{e}"))?;

    let canon = shortcut.to_string();
    let next = settings::Stored {
        hotkey: Some(canon.clone()),
    };
    // 存失败不影响本次注册，只是下次启动会回到默认值
    if let Err(e) = settings::save(&settings_path(&app), &next) {
        eprintln!("快捷键没存住，下次启动会回到默认值: {e}");
    }
    Ok(canon)
}

/// 读当前快捷键。没设过就返回平台默认值
#[tauri::command]
fn get_hotkey(app: tauri::AppHandle) -> String {
    let platform = hotkey::Platform::current();
    settings::load(&settings_path(&app))
        .hotkey
        .unwrap_or_else(|| hotkey::default_hotkey(platform).to_string())
}

// ── 入口 ─────────────────────────────────────────────────────────

/// 数据库文件位置。Tauri 的 app_data_dir 在各平台分别是
/// ~/.local/share、~/Library/Application Support、%APPDATA%
fn db_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("devclip.db")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    // 松开时也会来一次事件，忽略，否则调色板会开一下就关
                    if event.state == ShortcutState::Pressed {
                        toggle_palette(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let path = db_path(app.handle());
            let conn = db::open(&path)
                .unwrap_or_else(|e| panic!("打开数据库失败 {}: {e}", path.display()));
            app.manage(Db {
                conn: std::sync::Mutex::new(conn),
            });
            app.manage(RestoreTarget::default());

            let sw = Arc::new(SelfWrite::default());
            app.manage(sw.clone());
            clipboard::spawn_watcher(app.handle().clone(), sw);

            register_hotkey(app.handle());
            build_tray(app.handle()).unwrap_or_else(|e| {
                eprintln!("托盘图标创建失败：{e}");
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // 调色板是常驻后台的，关掉窗口只是收起，不是退出应用
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_items,
            get_item,
            toggle_favorite,
            remove_items,
            clear_all,
            add_item,
            item_count,
            hide_window,
            copy_to_clipboard,
            paste,
            available_actions,
            run_toolbox_action,
            get_settings,
            set_settings,
            set_hotkey,
            get_hotkey,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
