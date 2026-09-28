/**
 * Rust 侧 —— 阶段 4：SQLite + FTS5
 *
 * 与 src/lib/api.ts 的 ClipboardApi 一一对应。
 *
 * 分层：
 * - db.rs   连接、PRAGMA、schema、FTS5 虚表与触发器
 * - repo.rs 全部 SQL（upsert 去重、query 过滤、增删改）
 * - 本文件   只做「取连接 → 调 repo → 返回」，不写 SQL
 *
 * 字段名约定：serde rename_all = "camelCase"，
 * 与前端 api.ts 的 camelCase 字段对齐。
 */
// db / repo 设为 pub 是给 examples/seed.rs 用的：
// 它要直接调 upsert 灌数据，绕不过这层
pub mod db;
mod detect;
pub mod repo;

use db::DbError;
use repo::{ClipboardItem, Query};
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

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

#[tauri::command]
fn copy_to_clipboard(_id: i64) {
    // TODO: 阶段 5 写系统剪贴板
}

#[tauri::command]
fn paste(_id: i64) {
    // TODO: 阶段 5 模拟按键粘贴
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
        .setup(|app| {
            let path = db_path(app.handle());
            let conn = db::open(&path)
                .unwrap_or_else(|e| panic!("打开数据库失败 {}: {e}", path.display()));
            app.manage(Db {
                conn: std::sync::Mutex::new(conn),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_items,
            get_item,
            toggle_favorite,
            remove_items,
            clear_all,
            add_item,
            item_count,
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
