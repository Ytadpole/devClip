/**
 * Tauri 后端实现 —— 阶段 2
 *
 * 每个方法对应 lib.rs 里的一个 #[tauri::command]。
 * 参数和返回值走 serde，字段名 camelCase。
 * 阶段 2 的 Rust 侧返回硬编码假数据，阶段 4 换成 SQLite。
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
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

  async updateItem(id: number, content: string): Promise<ClipboardItem> {
    return invoke("update_item", { id, content });
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

  async setHotkey(accel: string): Promise<string> {
    return invoke("set_hotkey", { accel });
  },

  async monitorStatus() {
    return invoke<string | null>("monitor_status");
  },

  async openExternal(url: string) {
    return invoke("open_external", { url });
  },

  subscribe(onChanged: () => void, onNotice?: (text: string) => void): () => void {
    // listen 是异步的，而取消订阅必须能同步调用 —— 调用方拿到的
    // 就是一个普通函数。所以先把 unlisten 挂起来，等它 resolve 之后
    // 再决定是真取消还是立刻补取消（订阅过程中就被取消的情况）
    const unlisteners: UnlistenFn[] = [];
    let cancelled = false;
    const add = (p: Promise<UnlistenFn>) =>
      p.then((fn) => {
        if (cancelled) fn();
        else unlisteners.push(fn);
      })
      .catch(() => {
        // 浏览器里没有 Tauri 的事件通道。e2e 跑的就是这个环境，
        // 静默退化成「永不触发」即可，不该在控制台留红
      });

    add(listen("clipboard://changed", () => onChanged()));
    // 降级提示。模拟粘贴失败时后端会发这条 —— 内容已经在剪贴板里，
    // 但用户不知道「为什么没粘上」，所以这条必须能显示出来
    if (onNotice) {
      add(
        listen<{ text: string }>("clipboard://notice", (e) =>
          onNotice(e.payload.text),
        ),
      );
    }
    return () => {
      cancelled = true;
      unlisteners.forEach((fn) => fn());
    };
  },
};

/**
 * 收起面板窗口。Esc 在浏览器里没有窗口可收，所以走 backend.ts 分派，
 * 别处不直接引这里
 */
export const tauriWindow = {
  hide(): Promise<void> {
    return invoke("hide_window");
  },
  /** 进入窗口拖动。data-tauri-drag-region 由注入脚本处理；搜索框
   * 这类「点 vs 拖」要自己判阈值的元素，在移动超限后调这里 */
  startDragging(): Promise<void> {
    return invoke("plugin:window|start_dragging");
  },
};

/**
 * 托盘菜单「设置」→ 打开设置页。
 *
 * Rust 不直接操作 React，只发事件；这里负责把它接成回调。
 * 浏览器里没有事件通道，listen 会 reject，静默退化成「永不触发」
 */
export const tauriEvents = {
  onOpenSettings(cb: () => void): () => void {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;
    listen("settings://open", () => cb())
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  },
};

/**
 * 阶段 4 起可用：把一条内容写进真实数据库。
 *
 * 不在 ClipboardApi 里 —— 那是历史记录的读写契约，而入库是
 * 阶段 5「剪贴板监听」内部要用的入口，前端面板不直接用它。
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
