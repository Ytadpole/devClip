/**
 * DevClip · 前后端契约
 *
 * 这是前端与 Rust 之间**唯一**的接口定义。
 * 阶段 1 用 mock 实现，阶段 2 换成 Tauri 实现 —— 上层代码一行都不用改。
 * 对应文档：docs/02-架构设计.md
 */

export type ContentType =
  | "text"
  | "code"
  | "json"
  | "jwt"
  | "sql"
  | "url"
  | "uuid"
  | "base64"
  | "ip"
  | "exception"
  | "commit"
  | "markdown"
  | "image";

export interface ClipboardItem {
  id: number;
  content: string;
  contentType: ContentType;
  preview: string;
  imagePath?: string;
  byteSize: number;
  copyCount: number;
  sourceApp?: string;
  createdAt: number;
  lastCopiedAt: number;
  favorite: boolean;
  sensitive: boolean;
}

export interface Query {
  text?: string;
  types?: ContentType[];
  favoriteOnly?: boolean;
  /**
   * 含敏感信息的分组。默认关 —— 敏感项不参与任何普通结果，
   * 用户主动展开才可见（docs/03）。过滤在后端做，前端不碰
   */
  includeSensitive?: boolean;
  since?: number;
  limit?: number;
  offset?: number;
}

export interface ToolboxAction {
  id: string;
  label: string;
  /**
   * 一句话解释这个动作会得到什么。由 Rust 侧给，前端不猜。
   *
   * 存在的理由不只是「解释」：JWT 那几条必须带上
   * 「base64 不是加密」—— 用户点之前就该知道 payload 是明文可读的，
   * 而不是解完才发现。这句话写在后端是因为它跟着动作的实现走，
   * 写在文档里则没人会看到
   */
  hint?: string;
}

/**
 * 主题。`system` 跟随操作系统，但它不会原样传给 CSS ——
 * 前端解析成 dark / light 之后才落到 <html> 上（见 lib/theme.ts）
 */
export type Theme = "dark" | "light" | "system";

export interface Settings {
  hotkey: string;
  maxItems: number;
  retentionDays: number;
  maxImageBytes: number;
  theme: Theme;
  sensitiveAutoExpire: boolean;
}

/** 工具箱动作的输入输出约定：入参原文，出参结果 */
export type ActionResult = { ok: true; value: string } | { ok: false; error: string };

export interface ClipboardApi {
  list(q: Query): Promise<ClipboardItem[]>;

  get(id: number): Promise<ClipboardItem | null>;
  toggleFavorite(id: number): Promise<boolean>;
  remove(ids: number[]): Promise<void>;
  clearAll(): Promise<void>;

  copyToClipboard(id: number): Promise<void>;
  paste(id: number): Promise<void>;

  /**
   * 这个内容类型可用的工具箱动作。列表完全由后端决定 ——
   * 前端不做任何类型判断（见 AGENTS.md 的两咽喉点）
   */
  availableActions(t: ContentType): Promise<ToolboxAction[]>;

  /**
   * 跑一个动作。**结果是写进系统剪贴板的**，返回值只是一句摘要，
   * 用来显示在状态栏。
   *
   * 为什么不直接返回结果字符串：格式化后的 JSON 动辄几 KB，
   * 状态栏放不下也不该放。用户要的是「变换完直接粘」，
   * 结果在哪都比在界面上更好用
   */
  runToolboxAction(id: number, actionId: string): Promise<ActionResult>;

  getSettings(): Promise<Settings>;
  setSettings(patch: Partial<Settings>): Promise<Settings>;

  /**
   * 改全局快捷键。返回规范化后的串（如 `shift+alt+v`）。
   *
   * 单独一个方法而不塞进 setSettings：它有注册系统钩子这种
   * 立即生效的副作用，失败方式也不同（可能被其他应用占用）。
   * Err 的内容是能直接展示的中文
   */
  setHotkey(accel: string): Promise<string>;

  /**
   * 剪贴板监听是否可用。`Some(原因)` = 没起来，原因可直接展示给用户
   * （如 Wayland 会话没有剪贴板读取权限）。
   *
   * 单独问一次而不走事件：能不能监听在启动时就定了，而 webview 是
   * 那之后才加载的，事件发出去没人接
   */
  monitorStatus(): Promise<string | null>;

  /**
   * 订阅剪贴板变化。后端抓到新内容入库后会回调，调用方据此刷新列表。
   * 返回取消订阅的函数。
   *
   * `onNotice` 收后端的降级提示（如模拟粘贴失败、请手动粘贴）。
   * 没有它用户只看到「点了没反应」
   *
   * mock 端没有真实剪贴板，回调永不触发 —— 界面照常工作，
   * 不会因为缺事件而空转
   */
  subscribe(onChanged: () => void, onNotice?: (text: string) => void): () => void;
}
