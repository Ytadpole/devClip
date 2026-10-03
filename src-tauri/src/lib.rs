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

/// 面板弹出前的前台应用，粘贴时要回到那里。
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        // 快捷键的默认值是分平台的（docs/05：一个值套三个平台
        // 必有一个撞车），从 hotkey.rs 取，别在这里再写一份
        hotkey: hotkey::default_hotkey(hotkey::Platform::current()).to_string(),
        max_items: 1000,
        retention_days: 30,
        max_image_bytes: 10 * 1024 * 1024,
        theme: "dark".into(),
        sensitive_auto_expire: true,
    }
}

/// 设置的运行时快照。启动时从盘上读一次，之后命令只改这里；
/// 落盘只是快照的投影。监听线程与命令都要读它，
/// 单一事实来源免得两处各读各的文件
pub struct RuntimeSettings(std::sync::Mutex<Settings>);

impl RuntimeSettings {
    fn get(&self) -> Settings {
        unpoison(&self.0).clone()
    }
    fn set(&self, s: Settings) {
        *unpoison(&self.0) = s;
    }
}

/// settings::Stored（可缺字段）→ 完整 Settings，缺的用默认值补
fn stored_to_settings(s: settings::Stored) -> Settings {
    let d = default_settings();
    Settings {
        hotkey: s.hotkey.unwrap_or(d.hotkey),
        max_items: s.max_items.unwrap_or(d.max_items),
        retention_days: s.retention_days.unwrap_or(d.retention_days),
        max_image_bytes: s.max_image_bytes.unwrap_or(d.max_image_bytes),
        theme: s.theme.unwrap_or(d.theme),
        sensitive_auto_expire: s.sensitive_auto_expire.unwrap_or(d.sensitive_auto_expire),
    }
}

fn settings_to_stored(s: &Settings) -> settings::Stored {
    settings::Stored {
        hotkey: Some(s.hotkey.clone()),
        max_items: Some(s.max_items),
        retention_days: Some(s.retention_days),
        max_image_bytes: Some(s.max_image_bytes),
        theme: Some(s.theme.clone()),
        sensitive_auto_expire: Some(s.sensitive_auto_expire),
    }
}

/// 前端 patch 过来的界限。越界的值收进来而不是报错 ——
/// 设置页的控件本身有范围，这里只是防手改配置文件之外的极端值。
/// retention_days = 0 是合法语义（立即过期），不能当无效值挡掉
fn clamp_settings(s: &mut Settings) {
    s.max_items = s.max_items.clamp(10, 100_000);
    s.retention_days = s.retention_days.clamp(0, 3650);
    s.max_image_bytes = s.max_image_bytes.clamp(64 * 1024, 100 * 1024 * 1024);
    if !matches!(s.theme.as_str(), "dark" | "light" | "system") {
        s.theme = "dark".into();
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
            sensitive: false,
            expires_at: None,
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

/// 编辑原条目（docs/04 通用操作 Edit）。
///
/// 类型识别与敏感扫描都在这里对**新内容**重跑，前端只管把文本送来：
/// 前端自己复刻 detect/sensitive 会得到第二套判据，两边迟早对不上。
/// 编辑出来的密钥同样要走「入库打标 + 60 秒过期」，和复制进来的一视同仁；
/// 反过来把密钥改没了就回到普通条目，TTL 一并清掉。
///
/// 图片条目拒编辑：content 只是占位文本，真正的东西在文件里，
/// 改文本等于让缩略图和内容对不上
#[tauri::command]
fn update_item(
    state: State<Db>,
    rt: State<'_, RuntimeSettings>,
    id: i64,
    content: String,
) -> Result<ClipboardItem, String> {
    if content.trim().is_empty() {
        return Err("内容不能为空".into());
    }
    if content.len() > clipboard::MAX_BYTES {
        return Err("内容超过 1 MB 上限".into());
    }
    let sensitive = crate::sensitive::scan(&content).is_some();
    let expires_at = if sensitive && rt.get().sensitive_auto_expire {
        Some(repo::now_ms() + clipboard::SENSITIVE_TTL_MS)
    } else {
        None
    };
    let c = lock(&state)?;
    repo::update_content(&c, id, &content, sensitive, expires_at)
        .map_err(|e| err("保存修改失败", e))?
        .ok_or_else(|| "该条已被删除".to_string())
}

/// 收起面板。Esc 的第三级 —— 菜单没开、搜索框也是空的时候
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

/// 粘到弹出面板前的前台应用。
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

    let back = unpoison(&target.0).take();
    paste_into_restore_target(&app, back).await.map(|_| ())
}

/// 收面板 → 激活原目标窗口 → 发粘贴键。`paste` 与
/// `run_toolbox_action` 走的是同一条路，抽出来是为了让顺序只有一处
///
/// `back` 是弹出面板前的前台窗口，由调用方先 `take()` 出来 ——
/// 这个函数要跨 await，不能持有 `State` 的借用
///
/// 顺序不能换：先收起自己的窗口把焦点让出去，目标应用到前台之后
/// 再等它稳定，最后才发按键。少任何一步，按键都会落到 DevClip 身上
///
/// 返回值：`Ok(true)` = 键发出去了；`Ok(false)` = 降级（已发提示，
/// 用户手动按一下即可）；`Err` = 连目标窗口都激活不了，面板已弹回
async fn paste_into_restore_target(
    app: &tauri::AppHandle,
    back: Option<String>,
) -> Result<bool, String> {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }

    if let Some(bundle) = back {
        if let Err(e) = clipboard::activate(&bundle) {
            // 窗口已经藏起来了，不还原的话用户只会看到面板凭空消失，
            // 报错文案根本没人看得见
            show_palette(app);
            return Err(e);
        }
        // 目标应用激活是异步的，图标弹回动画期间发键会被丢掉
        std::thread::sleep(std::time::Duration::from_millis(120));
    }
    // 降级路径（docs/05 风险 2）：模拟按键失败时**不能让功能死掉**。
    // 内容已经写进剪贴板了，所以至少还能让用户手动按一下 ——
    // 一个「偶尔要手动按 ^V」的版本，远好过一个「粘贴按钮点不动」的版本
    match clipboard::send_paste_keystroke() {
        Ok(()) => Ok(true),
        Err(e) => {
            // 窗口已经藏了、目标也激活了，不弹回来就没有任何提示，
            // 用户只会看到「点了没反应」
            show_palette(app);
            // 认得出是授权问题时随提示带上面板名，前端据此渲染
            // 「去授权」按钮 —— 把「照着文案找设置项」缩短成一次点击
            let action = clipboard::permission_pane(&e);
            // emit 而不是直接改 store：命令层拿不到 store，
            // 而前端已经在监听这个事件（api.ts 的 subscribe 同一条通道）
            let _ = tauri::Emitter::emit(
                app,
                "clipboard://notice",
                serde_json::json!({
                    "text": format!(
                        "已复制到剪贴板，请手动按 {} 粘贴",
                        clipboard::PASTE_KEY_HINT
                    ),
                    "reason": e,
                    "action": action,
                }),
            );
            Ok(false)
        }
    }
}

/// 打开 macOS 的授权面板（系统设置 → 隐私与安全性）。
///
/// 模拟按键被 TCC 拒掉时，降级提示里会带一个「去授权」按钮调这里。
/// `pane` 是 clipboard::permission_pane 给出的面板名
#[tauri::command]
fn open_permission_settings(pane: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let url = match pane.as_str() {
            "accessibility" => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
            "automation" => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation"
            }
            _ => return Err(format!("未知的授权面板：{pane}")),
        };
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("打开系统设置失败：{e}"))?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pane;
        Err("此平台无需授权".into())
    }
}

#[tauri::command]
fn available_actions(content_type: String) -> Vec<ToolboxAction> {
    // 注册表是唯一的事实来源。前端不认任何具体类型 ——
    // 加一类动作只改 toolbox/mod.rs 一处
    let t = content_type.parse().unwrap_or(detect::ContentType::Text);
    toolbox::for_type(t)
}

/// 用系统默认程序打开一条 URL
///
/// **只放行 http / https。** `url` 这个内容类型的判据里 scheme 包含
/// `file`、`ftp`、`ws`、`git@…` 等好几种（docs/04），而这里拿到的是
/// **用户复制来的任意文本** —— 直接交给 opener 就等于「点一下可能
/// 唤起任意已注册的程序」。`file://` 能读本地文件，`smb://` 能碰
/// 网络共享。只认 web 协议是这里唯一能守住的那条线。
#[tauri::command]
fn open_external(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let u = check_openable(&url)?;
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(u.as_str(), None::<String>)
        .map_err(|e| format!("打开失败：{e}"))
}

/// 能不能交给 opener。**纯函数，所以能测** —— 而这一条恰好是安全
/// 边界，不该只能靠手点验证
fn check_openable(url: &str) -> Result<url::Url, String> {
    let u = url::Url::parse(url.trim()).map_err(|e| format!("不是合法 URL：{e}"))?;
    if !matches!(u.scheme(), "http" | "https") {
        return Err(format!("只打开 http/https 链接，这个是 {}", u.scheme()));
    }
    Ok(u)
}

/// 跑一个工具箱动作，然后**直接粘到弹出面板前的前台窗口**。
/// 返回的是**一句摘要**，不是结果本身
///
/// 结果已经写进系统剪贴板了：工具箱的用处就是「变换完直接粘」，
/// 而面板状态栏只有一行、2.6 秒后自动消失 —— 塞不下格式化后的 JSON。
/// 写剪贴板前必须在 `SelfWrite` 记一笔，否则监听线程会把这个
/// 结果当成用户在别处复制的内容，又存一条
///
/// 必须是 async：粘的那几步有等待与外部进程调用，同步命令会跑在
/// 主线程上冻住事件循环（同 `paste`）。所以入参只有 `AppHandle` ——
/// async 命令带 `State<'_, T>` 引用入参编译不过（future 必须
/// `'static`），要拿状态就在 await 之前用 `app.state::<T>()`
#[tauri::command]
async fn run_toolbox_action(app: tauri::AppHandle, id: i64, action_id: String) -> ActionResult {
    let entry = match toolbox::find(&action_id) {
        Some(e) => e,
        None => return ActionResult::err(format!("没有这个动作：{action_id}")),
    };
    // 这段是同步的，全部状态都在 await 之前取完
    let (done, back) = {
        let db = app.state::<Db>();
        let sw = app.state::<Arc<SelfWrite>>();

        let item = match repo::get(&unpoison(&db.conn), id) {
            Ok(Some(it)) => it,
            Ok(None) => return ActionResult::err(format!("第 {id} 条已不在历史里")),
            Err(e) => return ActionResult::err(format!("读取失败：{e}")),
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
        let value = match (entry.run)(&item.content) {
            Ok(v) => v,
            Err(e) => return ActionResult::err(e),
        };
        sw.mark(&value);
        if let Err(e) = clipboard::write_text(&value) {
            return ActionResult::err(format!("动作成功但写不回剪贴板：{e}"));
        }
        let done = format!(
            "{}：{} → {}",
            entry.label,
            human(item.content.len()),
            human(value.len())
        );
        let target = app.state::<RestoreTarget>();
        let back = unpoison(&target.0).take();
        (done, back)
    };

    // 降级时（键没发出去）不说「已粘贴」—— notice 已经告诉用户手动
    // 按哪一下了，摘要再报一次「已粘贴」会与事实矛盾
    match paste_into_restore_target(&app, back).await {
        Ok(true) => ActionResult::ok(format!("{done}，已粘贴")),
        Ok(false) => ActionResult::ok(format!("{done}，已复制到剪贴板")),
        Err(e) => ActionResult::err(format!("{done}，但没能粘贴：{e}")),
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

/// 读设置。返回快照 —— 盘上的文件只在启动时读一次
#[tauri::command]
fn get_settings(rt: State<'_, RuntimeSettings>) -> Settings {
    rt.get()
}

/// set_settings 收到的部分 patch。TS 侧是 Partial<Settings>
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    hotkey: Option<String>,
    max_items: Option<i64>,
    retention_days: Option<i64>,
    max_image_bytes: Option<i64>,
    theme: Option<String>,
    sensitive_auto_expire: Option<bool>,
}

/// 合并 patch 并落盘，返回合并后的完整设置。
///
/// 快捷键走 apply_hotkey（要真的注册），其余字段纯数据。
/// hotkey 注册失败则整体失败 —— 一半生效一半没生效的设置
/// 比全都失败更难排查
#[tauri::command]
fn set_settings(
    app: tauri::AppHandle,
    rt: State<'_, RuntimeSettings>,
    patch: SettingsPatch,
) -> Result<Settings, String> {
    let mut next = rt.get();
    if let Some(hk) = &patch.hotkey {
        apply_hotkey(&app, hk)?;
        next.hotkey = hk.to_string();
    }
    if let Some(v) = patch.max_items {
        next.max_items = v;
    }
    if let Some(v) = patch.retention_days {
        next.retention_days = v;
    }
    if let Some(v) = patch.max_image_bytes {
        next.max_image_bytes = v;
    }
    if let Some(v) = patch.theme {
        next.theme = v;
    }
    if let Some(v) = patch.sensitive_auto_expire {
        next.sensitive_auto_expire = v;
    }
    clamp_settings(&mut next);

    rt.set(next.clone());
    if let Err(e) = settings::save(&settings_path(&app), &settings_to_stored(&next)) {
        // 本次已生效，只是下次启动会回到旧值。别让用户重填一遍
        eprintln!("设置没存住，重启后会回到旧值: {e}");
    }
    Ok(next)
}

// ── 过期清理后台任务 ─────────────────────────────────────────────

/// 清理检查间隔。敏感 TTL 是 60 秒，5 秒一轮足以让「到期即消失」
/// 没有可感知的延迟，也远谈不上 CPU 开销
const EXPIRY_CHECK: std::time::Duration = std::time::Duration::from_secs(5);

/// 跑一轮清理：删掉已到期条目；剪贴板里躺的正是刚删内容时把它
/// 一并清空；有删除才通知前端刷新。返回删掉的条数
fn purge_tick(app: &tauri::AppHandle) -> usize {
    let removed = {
        let state = app.state::<Db>();
        let conn = unpoison(&state.conn);
        repo::purge_expired(&conn, repo::now_ms()).unwrap_or_else(|e| {
            eprintln!("过期清理失败：{e}");
            vec![]
        })
    };
    if removed.is_empty() {
        return 0;
    }
    // 数据库锁上面已经还了：剪贴板是系统调用，慢了也不能拖住别的命令。
    // 比对在本地做，不用再回数据库
    if let Some(cur) = clipboard::read_text() {
        if removed.iter().any(|s| s == &cur) {
            if let Err(e) = clipboard::clear_text() {
                eprintln!("{e}");
            }
        }
    }
    // 载荷无所谓，前端订阅方只拿它当「该刷新了」的信号
    let _ = tauri::Emitter::emit(app, "clipboard://changed", serde_json::json!({}));
    removed.len()
}

/// 后台清理线程。启动先清一轮 —— 应用没在跑的期间到期的条目
/// （比如上次退出前刚复制的密钥）不该等到下一个 5 秒才消失，
/// 更不该在启动后的整个会话里一直留着
fn spawn_expiry_task(app: tauri::AppHandle) {
    std::thread::Builder::new()
        .name("devclip-expiry".into())
        .spawn(move || loop {
            purge_tick(&app);
            std::thread::sleep(EXPIRY_CHECK);
        })
        .expect("起过期清理线程失败");
}

// ── 窗口与快捷键 ─────────────────────────────────────────────────/// 让面板显形。窗口启动时是隐藏的（见 tauri.conf.json），
/// 全靠快捷键呼出来
/// 用户拖动后的窗口位置（物理坐标）。只在内存里记：应用常驻，这个
/// 状态覆盖绝大多数场景；跨重启回到居中，要跨重启再加进 settings。
static LAST_WIN_POS: std::sync::Mutex<Option<(i32, i32)>> = std::sync::Mutex::new(None);

/// 让面板拿到键盘焦点。只对**当前可见**的面板有意义。
///
/// Linux 上要走两条路，`set_focus()` 单独一条不够。
///
/// `set_focus()` 走 GTK 的 present 路径，在 Muffin（Cinnamon 的 WM）
/// 上会被防焦点抢占策略整个拒掉：`focus-new-windows` 是 `smart`，
/// 而托盘应用从来没有用户输入事件，`_NET_WM_USER_TIME` 属性压根
/// 不存在，请求一律不理。实测面板 `IsViewable` 而焦点纹丝不动，
/// 连测 6 轮 6 次没拿到过。
///
/// **要命的不是「焦点拿得慢」，是永远拿不到。** 没有 FocusIn
/// 就永远等不到 FocusOut，而 `WindowEvent::Focused(false)` 是
/// 唯一的收起路径 —— 面板会变成一个打不了字、也关不掉的东西。
///
/// EWMH 的 `_NET_ACTIVE_WINDOW` 客户消息是 WM 认可的路子，
/// 实测 source indication 填 1 或 2 都放行，用的是 paste 前唤醒
/// 目标窗口那同一个函数。
///
/// ## 开头那个可见性判断不能省
///
/// `set_focus()` 在 GTK 里是 `gtk_window_present()`，而 present
/// 会**把隐藏的窗口重新映射出来**。抢焦点的轮询最长活 2s，用户
/// 在这期间点走让面板收起，下一次重试就会把刚藏好的面板又掏出来，
/// 而 Tauri 那边仍记着「已隐藏」—— 面板就此卡住：X 认为它可见、
/// 快捷键认为它隐藏，两边对不上，点它没反应、快捷键也切不走。
///
/// 这不是假设，是本机实测到的状态。挡住它的就是这一行。
///
/// macOS / Windows 上 `set_focus()` 本来就好使，不发这条消息
#[cfg(target_os = "linux")]
fn request_focus(w: &tauri::WebviewWindow) {
    if !matches!(w.is_visible(), Ok(true)) {
        return;
    }
    let _ = w.set_focus();
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = w.window_handle() else {
        return;
    };
    // wry 在 Linux 上给的是 Xlib。两个变体的字段类型不一样
    // （Xlib 是 u64，Xcb 是 NonZero<u32>），X11 窗口号本身只有
    // 32 位，超出范围说明拿到的不是 X11 句柄，直接放弃
    let wide = match handle.as_raw() {
        RawWindowHandle::Xlib(h) => h.window,
        RawWindowHandle::Xcb(h) => h.window.get().into(),
        _ => return,
    };
    let Ok(win) = u32::try_from(wide) else {
        return;
    };
    // 失败就算了：面板刚 show 出来时还没进 VIEWABLE，这个函数会
    // 提前返回错误，下面那轮轮询会再试
    let _ = clipboard::linux::activate_window(win);
}

#[cfg(not(target_os = "linux"))]
fn request_focus(w: &tauri::WebviewWindow) {
    let _ = w.set_focus();
}

fn show_palette(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        // 用户拖过的位置优先；没拖过才居中。配置里的 center: true 对
        // 无边框窗口不可靠（实测落点漂移甚至顶到左上角），不能依赖
        let pos = *LAST_WIN_POS.lock().unwrap_or_else(|e| e.into_inner());
        match pos {
            Some((x, y)) => {
                let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
            }
            None => {
                let _ = w.center();
            }
        }
        let _ = w.show();
        // 抢焦点这件事 WM 会间歇性吞掉。轮询确认制：每 100ms 查
        // is_focused，没拿到就再要，最多 ~2s——不依赖事件运气。
        // 每次重试都走 request_focus 而不是裸 set_focus：Linux 上
        // 只有客户消息那条路管用
        //
        // **show 之后不要立刻发激活请求**，第一轮要等 100ms。
        // 那时 WM 还没处理完映射，激活请求会让它认为这个客户端要
        // 自己管这个窗口，于是按自己的规则重新摆放 —— 实测面板被
        // 丢到 (1022,574)，大半个在屏幕外，center() 白写了。
        // 等一拍再发，它就只做聚焦、不碰位置
        let w2 = w.clone();
        std::thread::spawn(move || {
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                // 面板已经被收起就收手。request_focus 自己也会查，
                // 但在这里就退出更早：留着这条线程继续重试毫无意义
                if !matches!(w2.is_visible(), Ok(true)) {
                    return;
                }
                if w2.is_focused().unwrap_or(false) {
                    return;
                }
                request_focus(&w2);
            }
        });
    }
}

/// 失焦后推迟一拍再隐藏。
///
/// 直接 hide 会有一个很难查的竞态：全局快捷键是靠 X11 的被动 grab
/// 实现的，grab 在**按下那一刻**激活，而 grab 属于另一个客户端，
/// WM 会为此先发一次 FocusOut —— 实测它比快捷键处理函数早到十几
/// 毫秒。于是 [toggle_palette] 执行时面板已经被这次 FocusOut 收起，
/// 它以为当前是收起态，又把面板显示回来。表现就是「面板开着的时候
/// 按快捷键，一点反应都没有」。
///
/// 真的失焦不会自己回来，150ms 后再确认一次就能把两者分开。
/// 代价是收起比点击晚 150ms，手感上察觉不到。
///
/// hide 走 `run_on_main_thread`：GTK 不是线程安全的，而这里是
/// 后台线程。实测从后台线程直接调 `hide()` 会在屏幕上留下一条
/// 十几像素高的残影（面板主体确实消失了，但顶部搜索行那条没擦掉）
fn hide_unless_refocused(window: tauri::Window) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        if matches!(window.is_focused(), Ok(true)) {
            return;
        }
        let app = window.app_handle().clone();
        let _ = app.run_on_main_thread(move || {
            let _ = window.hide();
        });
    });
}

/// 快捷键的显隐切换。记下前台应用：用户多半是从别的应用按快捷键过来的，
/// 粘贴时要回到那里。自己的 bundle id 要排除掉 —— 从 Dock 点开应用后
/// 我们会是前台，此时按快捷键如果把 DevClip 自己记进去，
/// 粘贴时就会激活自己，⌘V 按在面板上
fn toggle_palette(app: &tauri::AppHandle) {
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    if matches!(w.is_visible(), Ok(true)) {
        let _ = w.hide();
        return;
    }
    // 必须在 show_palette 之前取：面板一旦显示就会抢到焦点，
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
/// 存的是**窗口 ID**（X11）、**bundle id**（macOS）或 **HWND**（Windows），
/// 不是显示用的名字。名字会变、可能重复，靠它匹配回去经常落空
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
        clipboard::windows::frontmost_window().map(|h| format!("0x{h:x}"))
    }
}

/// 这是不是 DevClip 自己的窗口。
///
/// 面板呼出时会抢到焦点，而 X11 上 `XSetInputFocus` 之后
/// `_NET_ACTIVE_WINDOW` 指向的就是我们自己。不排除的话会出现
/// 两个后果：`source_app` 记成「DevClip」，以及粘贴时试图
/// 唤起 DevClip 自己，⌘V/^V 全打在面板上
fn is_own_window(app: &tauri::AppHandle, target: &str) -> bool {
    let Some(w) = app.get_webview_window("main") else {
        return false;
    };
    let own: String = w.title().unwrap_or_default().to_string();
    // 窗口标题是判断依据之一，但平台各异；真正稳的是配置里的
    // identifier —— 两个都查，任一命中就算自己
    if own == target || target == app.config().identifier {
        return true;
    }
    // Windows 的 target 是前台 HWND 的十六进制（0x…），标题与
    // identifier 都对不上，要拿自己主窗口的句柄比一遍才算数
    #[cfg(target_os = "windows")]
    if let Ok(h) = w.hwnd() {
        if target == format!("0x{:x}", h.0 as usize) {
            return true;
        }
    }
    false
}

/// 「这台机器不能监听」的原因，没有就是正常
///
/// 原来是发 `clipboard://unavailable` 事件，前端也确实没有监听方 ——
/// 但那不是漏写监听的问题，而是**这个事件永远送不到**：状态在
/// `setup()` 里就定了，而 webview 是那之后才加载的。所以改成可查询的
/// 状态，前端 `init()` 时问一次。
#[derive(Default)]
pub struct MonitorIssue(pub std::sync::Mutex<Option<String>>);

/// 前端问「监听起来了吗」。`Some(原因)` = 没起来，原因可直接展示
#[tauri::command]
fn monitor_status(issue: State<'_, MonitorIssue>) -> Option<String> {
    unpoison(&issue.0).clone()
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
/// 快捷键值从运行时快照读（setup 里快照先于这里就绪）。
/// 早先每次启动都用默认值，用户改了也没用 —— 文档里那一项
/// 写得再清楚也没意义
fn register_hotkey(app: &tauri::AppHandle) {
    let platform = hotkey::Platform::current();
    let spec = app.state::<RuntimeSettings>().get().hotkey;

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

    let show = MenuItem::with_id(app, "show", "显示面板", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 DevClip", true, None::<&str>)?;
    // 菜单里明说退出意味着什么。X11 上用户不知道「退出 = 剪贴板
    // 里没被存下的东西会丢」，而这正是最该说清的一句
    let menu = Menu::with_items(app, &[&show, &settings, &quit])?;

    let mut builder = TrayIconBuilder::with_id("devclip")
        .menu(&menu)
        .tooltip("DevClip — 剪贴板历史")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_palette(app),
            // 设置页是前端的一份状态，托盘只负责把窗口带出来并
            // 告诉前端「该切视图了」—— Rust 不直接操作 React
            "settings" => {
                show_palette(app);
                let _ = tauri::Emitter::emit(app, "settings://open", ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        // 左键直接开面板，比「右键才出菜单」少一次点击
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

/// 注册一个新快捷键（先注销旧的）。
///
/// 只管注册，不管持久化 —— 落盘由调用方随其他字段一起做，
/// 免得 set_hotkey 和 set_settings 各写一份文件。返回规范化后的值。
/// 前端会直接显示 Err 的内容，所以文案要能照着做 ——
/// 尤其是「已被占用」和「格式不对」必须分开说
fn apply_hotkey(app: &tauri::AppHandle, accel: &str) -> Result<String, String> {
    let platform = hotkey::Platform::current();
    let shortcut: Shortcut = accel
        .parse()
        .map_err(|_| format!("无法识别这个快捷键：{accel}。用 Alt+Shift+V 这种写法"))?;

    hotkey::check(platform, &shortcut)?;

    let gs = app.global_shortcut();
    // 先注销旧的再注册新的。不注销的话平台会报「已被占用」，
    // 而占用者其实是我们自己上一个组合
    let old = app.state::<RuntimeSettings>().get().hotkey;
    if let Ok(old_hk) = old.parse::<Shortcut>() {
        let _ = gs.unregister(old_hk);
    }

    gs.register(shortcut)
        .map_err(|e| format!("快捷键注册失败：{e}"))?;
    Ok(shortcut.to_string())
}

/// 改快捷键。返回规范化后的值
#[tauri::command]
fn set_hotkey(app: tauri::AppHandle, accel: String) -> Result<String, String> {
    let canon = apply_hotkey(&app, &accel)?;

    let rt = app.state::<RuntimeSettings>();
    let mut s = rt.get();
    s.hotkey = canon.clone();
    rt.set(s.clone());
    // 存失败不影响本次注册，只是下次启动会回到旧值
    if let Err(e) = settings::save(&settings_path(&app), &settings_to_stored(&s)) {
        eprintln!("快捷键没存住，下次启动会回到旧值: {e}");
    }
    Ok(canon)
}

/// 读当前快捷键。快照里已经是「设过的或平台默认值」
#[tauri::command]
fn get_hotkey(rt: State<'_, RuntimeSettings>) -> String {
    rt.get().hotkey
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
        // 必须是第一个注册的插件：第二个实例的启动参数要尽早被拦下
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // 这段跑在**已有**实例的进程里。用户再开一次 DevClip，
            // 意图就是要看到它 —— 把面板弹到前面即可，
            // 两个实例各开一个数据库才真的是灾难
            show_palette(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    // 松开时也会来一次事件，忽略，否则面板会开一下就关
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
            // 快照必须先于 register_hotkey 就绪：它要从这里读快捷键
            let settings_full = stored_to_settings(settings::load(&settings_path(app.handle())));
            app.manage(RuntimeSettings(std::sync::Mutex::new(settings_full)));

            let sw = Arc::new(SelfWrite::default());
            app.manage(sw.clone());
            // 必须在 spawn_watcher 之前 manage：监听起不来时它要往里写
            // 原因，而 try_state 对未 manage 的类型会失败
            app.manage(MonitorIssue::default());
            clipboard::spawn_watcher(app.handle().clone(), sw);

            spawn_expiry_task(app.handle().clone());

            register_hotkey(app.handle());
            build_tray(app.handle()).unwrap_or_else(|e| {
                eprintln!("托盘图标创建失败：{e}");
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // 拖动结束的落点记下来，下次呼出还回原位
            if let tauri::WindowEvent::Moved(pos) = event {
                if let Ok(mut slot) = LAST_WIN_POS.lock() {
                    *slot = Some((pos.x, pos.y));
                }
            }
            // 面板是常驻后台的，关掉窗口只是收起，不是退出应用
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
            // 点到面板外面就收起 —— Spotlight/Raycast 的标准行为。
            // 粘贴流程会先显式 hide 再切走焦点，这里再收一次是无害的
            // no-op；托盘「设置…」在失焦之后仍会走到 show_palette，
            // 所以从托盘进设置不受影响
            if let tauri::WindowEvent::Focused(false) = event {
                hide_unless_refocused(window.clone());
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_items,
            get_item,
            toggle_favorite,
            remove_items,
            clear_all,
            add_item,
            update_item,
            item_count,
            hide_window,
            copy_to_clipboard,
            paste,
            available_actions,
            open_external,
            open_permission_settings,
            run_toolbox_action,
            get_settings,
            set_settings,
            monitor_status,
            set_hotkey,
            get_hotkey,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 这一组守的是安全边界：`url` 内容类型的判据里 scheme 有好几种
    /// （docs/04），而传进来的是**用户复制来的任意文本**
    #[test]
    fn open_allows_only_web_schemes() {
        for ok in ["https://example.com/a?b=1", "http://127.0.0.1:8080/x"] {
            assert!(check_openable(ok).is_ok(), "{ok} 应当放行");
        }
        for no in [
            "file:///etc/passwd",
            "ftp://example.com/x",
            "smb://host/share",
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
        ] {
            let err = check_openable(no).unwrap_err();
            assert!(err.contains("http/https"), "{no} 应被拒，实际：{err}");
        }
    }

    /// 复制来的 URL 前后常有空白与换行（终端里尤其常见）
    #[test]
    fn open_trims_surrounding_space() {
        assert!(check_openable("  https://example.com\n").is_ok());
    }

    #[test]
    fn open_reports_garbage_as_not_a_url() {
        let err = check_openable("不是 URL").unwrap_err();
        assert!(err.contains("不是合法 URL"), "实际：{err}");
    }
}
