/**
 * 后端实现选择 —— 全应用唯一一处「走 mock 还是走 Rust」的分叉点
 *
 * 阶段 1  只有 mock
 * 阶段 2  新增 ./tauri 实现后，注册进 impls 即可，
 *         上层（store / 组件）一行都不用改。
 */

import { isTauri } from "@tauri-apps/api/core";
import type { ClipboardApi } from "./api";
import { mockApi } from "./mock";
import { tauriApi, tauriWindow } from "./tauri";

const impls: Record<string, ClipboardApi> = {
  mock: mockApi,
  tauri: tauriApi,
};

/**
 * 默认按运行环境挑：在 Tauri webview 里就用真后端，否则用 mock。
 *
 * 早先这里写死 `|| "mock"`，结果 `pnpm tauri dev` 起来的真应用也走 mock ——
 * Rust 那套剪贴板/数据库一行都不会被执行。靠 .env 也不合适：
 * e2e 跑在浏览器里，一旦全局设成 tauri，invoke 全都会 reject。
 * 运行时探测才两种场景都对
 */
export const backendName =
  import.meta.env.VITE_BACKEND || (isTauri() ? "tauri" : "mock");

export const api: ClipboardApi = impls[backendName] ?? mockApi;

/**
 * 收起调色板窗口。
 *
 * 不塞进 ClipboardApi：那个契约讲的是剪贴板历史，窗口显隐是另一回事。
 * 但分派仍然留在这个文件里 —— 组件不直接引 ./tauri
 */
export const hideWindow: () => Promise<void> =
  backendName === "tauri" ? tauriWindow.hide : () => Promise.resolve();
