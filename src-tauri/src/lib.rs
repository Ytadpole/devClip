/**
 * Rust 侧 —— 阶段 2：打通 IPC
 *
 * 与 src/lib/api.ts 的 ClipboardApi 一一对应。
 * 阶段 2 返回硬编码假数据，阶段 4 换成 SQLite + FTS5。
 *
 * 字段名约定：serde rename_all = "camelCase"，
 * 与前端 api.ts 的 camelCase 字段对齐。
 */

use serde::{Deserialize, Serialize};

// ── 与 api.ts 对应的类型 ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardItem {
    pub id: i64,
    pub content: String,
    pub content_type: String,
    pub preview: String,
    pub image_path: Option<String>,
    pub byte_size: i64,
    pub copy_count: i64,
    pub source_app: Option<String>,
    pub created_at: i64,
    pub last_copied_at: i64,
    pub favorite: bool,
    pub sensitive: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Query {
    pub text: Option<String>,
    pub types: Option<Vec<String>>,
    pub favorite_only: Option<bool>,
    pub since: Option<i64>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolboxAction {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Serialize)]
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
#[derive(Debug, Serialize)]
pub struct ActionResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ActionResult {
    pub fn ok(v: impl Into<String>) -> Self {
        ActionResult { ok: true, value: Some(v.into()), error: None }
    }
    pub fn err(e: impl Into<String>) -> Self {
        ActionResult { ok: false, value: None, error: Some(e.into()) }
    }
}

// ── 硬编码假数据 ─────────────────────────────────────────────────

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn sample_items() -> Vec<ClipboardItem> {
    let t = now();
    let m = 60_000;
    vec![
        ClipboardItem {
            id: 1,
            content: r#"{"name":"andy","age":18,"tags":["dev","rust"],"active":true}"#.into(),
            content_type: "json".into(),
            preview: r#"{"name":"andy","age":18,"tags":["dev","rust"],"active":true}"#.into(),
            image_path: None,
            byte_size: 60,
            copy_count: 3,
            source_app: Some("VS Code".into()),
            created_at: t - 2 * m,
            last_copied_at: t - 2 * m,
            favorite: true,
            sensitive: false,
        },
        ClipboardItem {
            id: 2,
            content: "select * from users where id = 1".into(),
            content_type: "sql".into(),
            preview: "select * from users where id = 1".into(),
            image_path: None,
            byte_size: 32,
            copy_count: 1,
            source_app: Some("DataGrip".into()),
            created_at: t - 5 * m,
            last_copied_at: t - 5 * m,
            favorite: false,
            sensitive: false,
        },
        ClipboardItem {
            id: 3,
            content: "https://github.com/tauri-apps/tauri".into(),
            content_type: "url".into(),
            preview: "https://github.com/tauri-apps/tauri".into(),
            image_path: None,
            byte_size: 38,
            copy_count: 2,
            source_app: Some("Firefox".into()),
            created_at: t - 10 * m,
            last_copied_at: t - 10 * m,
            favorite: true,
            sensitive: false,
        },
        ClipboardItem {
            id: 4,
            content: "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.sig".into(),
            content_type: "jwt".into(),
            preview: "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.sig".into(),
            image_path: None,
            byte_size: 149,
            copy_count: 1,
            source_app: Some("Postman".into()),
            created_at: t - 18 * m,
            last_copied_at: t - 18 * m,
            favorite: false,
            sensitive: true,
        },
        ClipboardItem {
            id: 5,
            content: "docker ps -a --format 'table {{.Names}}\\t{{.Status}}'".into(),
            content_type: "code".into(),
            preview: "docker ps -a --format 'table {{.Names}}\\t{{.Status}}'".into(),
            image_path: None,
            byte_size: 48,
            copy_count: 5,
            source_app: Some("Windows Terminal".into()),
            created_at: t - 24 * m,
            last_copied_at: t - 24 * m,
            favorite: false,
            sensitive: false,
        },
    ]
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

#[tauri::command]
fn list_items(_q: Query) -> Vec<ClipboardItem> {
    // TODO: 阶段 4 用 SQLite FTS5 做真实过滤
    sample_items()
}

#[tauri::command]
fn get_item(id: i64) -> Option<ClipboardItem> {
    sample_items().into_iter().find(|i| i.id == id)
}

#[tauri::command]
fn toggle_favorite(_id: i64) -> bool {
    // TODO: 阶段 4 持久化
    true
}

#[tauri::command]
fn remove_items(_ids: Vec<i64>) {
    // TODO: 阶段 4 持久化
}

#[tauri::command]
fn clear_all() {
    // TODO: 阶段 4 持久化
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
            ToolboxAction { id: "json.format".into(), label: "Format".into() },
            ToolboxAction { id: "json.minify".into(), label: "Minify".into() },
            ToolboxAction { id: "json.sort_keys".into(), label: "Sort Keys".into() },
        ],
        "jwt" => vec![
            ToolboxAction { id: "jwt.decode_header".into(), label: "Decode Header".into() },
            ToolboxAction { id: "jwt.decode_payload".into(), label: "Decode Payload".into() },
        ],
        "sql" => vec![
            ToolboxAction { id: "sql.format".into(), label: "Format".into() },
            ToolboxAction { id: "sql.tables".into(), label: "Extract Tables".into() },
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            list_items,
            get_item,
            toggle_favorite,
            remove_items,
            clear_all,
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
