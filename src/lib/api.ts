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
  since?: number;
  limit?: number;
  offset?: number;
}

export interface ToolboxAction {
  id: string;
  label: string;
}

export interface Settings {
  hotkey: string;
  maxItems: number;
  retentionDays: number;
  maxImageBytes: number;
  theme: "dark" | "light" | "system";
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

  availableActions(t: ContentType): Promise<ToolboxAction[]>;
  runToolboxAction(id: number, actionId: string): Promise<ActionResult>;

  getSettings(): Promise<Settings>;
  setSettings(patch: Partial<Settings>): Promise<Settings>;

  /**
   * 订阅剪贴板变化。后端抓到新内容入库后会回调，调用方据此刷新列表。
   * 返回取消订阅的函数。
   *
   * mock 端没有真实剪贴板，回调永不触发 —— 界面照常工作，
   * 不会因为缺事件而空转
   */
  subscribe(onChanged: () => void): () => void;
}
