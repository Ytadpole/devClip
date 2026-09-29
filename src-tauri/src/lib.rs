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
pub mod repo;
mod sensitive;

use clipboard::SelfWrite;
use db::DbError;
use repo::{ClipboardItem, Query};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

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
pub struct ToolboxAction {
    pub id: String,
    pub label: String,
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
    clipboard::send_paste_keystroke()
}

#[tauri::command]
fn available_actions(content_type: String) -> Vec<ToolboxAction> {
    // TODO: 阶段 6 从动作注册表动态返回
    match content_type.as_str() {
        "json" => vec![
            ToolboxAction {
                id: "json.format".into(),
                label: "Format".into(),
            },
            ToolboxAction {
                id: "json.minify".into(),
                label: "Minify".into(),
            },
            ToolboxAction {
                id: "json.sort_keys".into(),
                label: "Sort Keys".into(),
            },
        ],
        "jwt" => vec![
            ToolboxAction {
                id: "jwt.decode_header".into(),
                label: "Decode Header".into(),
            },
            ToolboxAction {
                id: "jwt.decode_payload".into(),
                label: "Decode Payload".into(),
            },
        ],
        "sql" => vec![
            ToolboxAction {
                id: "sql.format".into(),
                label: "Format".into(),
            },
            ToolboxAction {
                id: "sql.tables".into(),
                label: "Extract Tables".into(),
            },
        ],
        _ => vec![],
    }
}

#[tauri::command]
fn run_toolbox_action(_id: i64, action_id: String) -> ActionResult {
    // TODO: 阶段 6 实现具体动作
    ActionResult::err(format!("阶段 2：动作 {} 尚未实现", action_id))
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
    if let Some(bundle) = clipboard::frontmost_app() {
        if bundle != app.config().identifier {
            *unpoison(&app.state::<RestoreTarget>().0) = Some(bundle);
        }
    }
    show_palette(app);
}

/// 注册全局快捷键。注册不上就把窗口放出来 ——
/// 多半是被别的应用抢占了或系统没给权限。这时用户如果还进不来，
/// 就只剩杀进程重装，比多弹一个窗口糟糕得多
fn register_hotkey(app: &tauri::AppHandle) {
    let hotkey = default_settings().hotkey;
    let Ok(shortcut) = hotkey.parse::<Shortcut>() else {
        eprintln!("快捷键 {hotkey} 无法解析，窗口改为常驻显示");
        return show_palette(app);
    };
    if let Err(e) = app.global_shortcut().register(shortcut) {
        eprintln!("快捷键 {hotkey} 注册失败（{e}），窗口改为常驻显示");
        show_palette(app);
    }
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
