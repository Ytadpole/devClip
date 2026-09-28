/**
 * Tauri 后端实现 —— 阶段 2
 *
 * 每个方法对应 lib.rs 里的一个 #[tauri::command]。
 * 参数和返回值走 serde，字段名 camelCase。
 * 阶段 2 的 Rust 侧返回硬编码假数据，阶段 4 换成 SQLite。
 */

import { invoke } from "@tauri-apps/api/core";
import type {
  ActionResult,
  ClipboardApi,
  ClipboardItem,
  ContentType,
  Query,
  Settings,
  ToolboxAction,
} from "./api";

export const tauriApi: ClipboardApi = {
  async list(q: Query): Promise<ClipboardItem[]> {
    return invoke("list_items", { q });
  },

  async get(id: number): Promise<ClipboardItem | null> {
    return invoke("get_item", { id });
  },

  async toggleFavorite(id: number): Promise<boolean> {
    return invoke("toggle_favorite", { id });
  },

  async remove(ids: number[]): Promise<void> {
    return invoke("remove_items", { ids });
  },

  async clearAll(): Promise<void> {
    return invoke("clear_all");
  },

  async copyToClipboard(id: number): Promise<void> {
    await invoke("copy_to_clipboard", { id });
  },

  async paste(id: number): Promise<void> {
    await invoke("paste", { id });
  },

  async availableActions(t: ContentType): Promise<ToolboxAction[]> {
    return invoke("available_actions", { contentType: t });
  },

  async runToolboxAction(id: number, actionId: string): Promise<ActionResult> {
    return invoke("run_toolbox_action", { id, actionId });
  },

  async getSettings(): Promise<Settings> {
    return invoke("get_settings");
  },

  async setSettings(patch: Partial<Settings>): Promise<Settings> {
    return invoke("set_settings", { patch });
  },
};

/**
 * 阶段 4 起可用：把一条内容写进真实数据库。
 *
 * 不在 ClipboardApi 里 —— 那是历史记录的读写契约，而入库是
 * 阶段 5「剪贴板监听」内部要用的入口，前端调色板不直接用它。
 * 单独挂在 tauriApi 上，等阶段 5 接上监听再收进 backend。
 */
export const tauriDb = {
  /** 入库并去重，返回最终那条记录 */
  add(content: string, sourceApp?: string): Promise<ClipboardItem> {
    return invoke("add_item", { content, sourceApp });
  },
  /** 当前历史条数，测试和调试用 */
  count(): Promise<number> {
    return invoke("item_count");
  },
};
