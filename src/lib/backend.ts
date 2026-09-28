/**
 * 后端实现选择 —— 全应用唯一一处「走 mock 还是走 Rust」的分叉点
 *
 * 阶段 1  只有 mock
 * 阶段 2  新增 ./tauri 实现后，注册进 impls 即可，
 *         上层（store / 组件）一行都不用改。
 */

import type { ClipboardApi } from "./api";
import { mockApi } from "./mock";
import { tauriApi } from "./tauri";

const impls: Record<string, ClipboardApi> = {
  mock: mockApi,
  tauri: tauriApi,
};

/** 可在 .env 里覆盖：VITE_BACKEND=tauri */
export const backendName = import.meta.env.VITE_BACKEND || "mock";

export const api: ClipboardApi = impls[backendName] ?? mockApi;
