/**
 * 全局状态（zustand）
 *
 * 注意：过滤逻辑放在 Rust/SQLite 侧（对应 FTS5），
 * 这里每次查询都重新调 api.list()，不去前端做本地过滤 ——
 * 这样阶段 2 接上真后端时行为完全一致。
 */

import { create } from "zustand";
import { api, backendName } from "./lib/backend";
import type { ClipboardItem, ContentType, ToolboxAction } from "./lib/api";

export const ALL_TYPES: ContentType[] = [
  "json",
  "sql",
  "code",
  "url",
  "jwt",
  "text",
  "uuid",
  "base64",
  "ip",
  "commit",
  "markdown",
  "exception",
  "image",
];

const clamp = (n: number, len: number) => (len === 0 ? 0 : Math.max(0, Math.min(n, len - 1)));

interface Status {
  text: string;
  kind: "ok" | "err";
}

interface State {
  items: ClipboardItem[];
  actions: ToolboxAction[];
  loading: boolean;
  query: string;
  types: ContentType[];
  favoriteOnly: boolean;
  selected: number;
  status: Status | null;

  init: () => Promise<void>;
  refresh: () => Promise<void>;
  setQuery: (q: string) => void;
  toggleType: (t: ContentType) => void;
  toggleFavoriteOnly: () => void;
  select: (i: number) => void;
  move: (d: number) => void;
  loadActions: (i: ClipboardItem) => Promise<void>;

  toggleFavorite: (id: number) => Promise<void>;
  copy: (id: number) => Promise<void>;
  paste: (id: number) => Promise<void>;
  remove: (id: number) => Promise<void>;
  runAction: (item: ClipboardItem, actionId: string) => Promise<void>;
  say: (text: string, kind?: Status["kind"]) => void;
  clearStatus: () => void;
}

let statusTimer: ReturnType<typeof setTimeout> | undefined;

export const useStore = create<State>((set, get) => ({
  items: [],
  actions: [],
  loading: false,
  query: "",
  types: [],
  favoriteOnly: false,
  selected: 0,
  status: null,

  async init() {
    set({ loading: true });
    await get().refresh();
    set({ loading: false });
  },

  async refresh() {
    const { query, types, favoriteOnly } = get();
    const items = await api.list({ text: query, types, favoriteOnly, limit: 200 });
    set({ items, selected: clamp(get().selected, items.length) });
  },

  setQuery(query) {
    set({ query, selected: 0 });
    void get().refresh();
  },

  toggleType(t) {
    const cur = get().types;
    set({
      types: cur.includes(t) ? cur.filter((x) => x !== t) : [...cur, t],
      selected: 0,
    });
    void get().refresh();
  },

  toggleFavoriteOnly() {
    set({ favoriteOnly: !get().favoriteOnly, selected: 0 });
    void get().refresh();
  },

  select(i) {
    set({ selected: i });
  },

  move(d) {
    set({ selected: clamp(get().selected + d, get().items.length) });
  },

  async loadActions(i) {
    set({ actions: await api.availableActions(i.contentType) });
  },

  async toggleFavorite(id) {
    const now = await api.toggleFavorite(id);
    set({ items: get().items.map((x) => (x.id === id ? { ...x, favorite: now } : x)) });
    get().say(now ? "已收藏" : "已取消收藏");
  },

  async copy(id) {
    await api.copyToClipboard(id);
    get().say("已复制到剪贴板");
  },

  async paste(id) {
    await api.paste(id);
    const items = await api.list({
      text: get().query,
      types: get().types,
      favoriteOnly: get().favoriteOnly,
      limit: 200,
    });
    set({ items, selected: clamp(get().selected, items.length) });
    get().say("已粘贴");
  },

  async remove(id) {
    await api.remove([id]);
    const items = await api.list({
      text: get().query,
      types: get().types,
      favoriteOnly: get().favoriteOnly,
      limit: 200,
    });
    set({ items, selected: clamp(get().selected, items.length) });
    get().say("已删除");
  },

  async runAction(item, actionId) {
    const r = await api.runToolboxAction(item.id, actionId);
    get().say(r.ok ? r.value : r.error, r.ok ? "ok" : "err");
  },

  say(text, kind = "ok") {
    clearTimeout(statusTimer);
    set({ status: { text, kind } });
    statusTimer = setTimeout(() => set({ status: null }), 2600);
  },

  clearStatus() {
    clearTimeout(statusTimer);
    set({ status: null });
  },
}));

export const backendLabel = backendName;
