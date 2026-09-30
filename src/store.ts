/**
 * 全局状态（zustand）
 *
 * 注意：过滤逻辑放在 Rust/SQLite 侧（对应 FTS5），
 * 这里每次查询都重新调 api.list()，不去前端做本地过滤 ——
 * 这样阶段 2 接上真后端时行为完全一致。
 */

import { create } from "zustand";
import { api, backendName } from "./lib/backend";
import type { ActionResult, ClipboardItem, ContentType, Settings, ToolboxAction } from "./lib/api";

/** 搜索防抖。docs/06 定的是 80ms：再小则每个击键都打一次后端，
 * 再大则能感觉到「搜索不跟手」。 */
const SEARCH_DEBOUNCE = 80;

/** 一次取多少条。虚拟列表吃得下更多，但再多就该由后端分页
 *  （阶段 4 的 offset），这里只是给 mock 和早期一个上限。 */
const PAGE_LIMIT = 200;

const clamp = (n: number, len: number) => (len === 0 ? 0 : Math.max(0, Math.min(n, len - 1)));

interface Status {
  text: string;
  /**
   * `warn` 是「没完全成功但不至于失败」：模拟粘贴被拒后
   * 内容已在剪贴板、让用户手动按一下就好。跟 `err` 分开是因为
   * 两者的紧急程度不同，用户不该把降级当故障
   */
  kind: "ok" | "warn" | "err";
}

/** 右键菜单的挂载点。用视口坐标，菜单本体 fixed 定位。 */
export interface MenuTarget {
  item: ClipboardItem;
  x: number;
  y: number;
}

interface State {
  items: ClipboardItem[];
  actions: ToolboxAction[];
  loading: boolean;
  query: string;
  types: ContentType[];
  favoriteOnly: boolean;
  /** 含敏感信息分组。默认关，见 api.ts Query 的说明 */
  sensitive: boolean;
  /** 调色板当前视图。设置页盖在列表上，Esc 退回 */
  view: "list" | "settings";
  /** 设置页的草稿来源。打开设置页时拉一次 */
  settings: Settings | null;
  selected: number;
  status: Status | null;
  menu: MenuTarget | null;

  init: () => Promise<void>;
  /** 变更之后重新拉取（收藏、粘贴、删除），尽量保住选中项 */
  refresh: () => Promise<void>;
  /** 筛选条件变了重新拉取，选中项回到第一条 */
  refilter: () => Promise<void>;
  setQuery: (q: string) => void;
  toggleType: (t: ContentType) => void;
  toggleFavoriteOnly: () => void;
  toggleSensitive: () => void;
  clearFilters: () => void;
  select: (i: number) => void;
  move: (d: number) => void;
  jump: (i: number) => void;
  loadActions: (t: ContentType) => Promise<void>;

  openSettings: () => void;
  closeSettings: () => void;
  saveSettings: (patch: Partial<Settings>) => Promise<void>;
  saveHotkey: (accel: string) => Promise<void>;

  toggleFavorite: (id: number) => Promise<void>;
  copy: (id: number) => Promise<void>;
  paste: (id: number) => Promise<void>;
  remove: (id: number) => Promise<void>;
  runAction: (item: ClipboardItem, actionId: string) => Promise<void>;
  say: (text: string, kind?: Status["kind"]) => void;

  openMenu: (item: ClipboardItem, x: number, y: number) => void;
  closeMenu: () => void;
}

let statusTimer: ReturnType<typeof setTimeout> | undefined;
let queryTimer: ReturnType<typeof setTimeout> | undefined;

/** 请求序号。mock 的延迟是固定的，看不出乱序；接上 SQLite 之后
 *  同一串查询的耗时会抖动，慢的旧响应可能后到并覆盖新结果。
 *  只有序号等于最新一次的请求才允许写回 state。 */
let reqSeq = 0;

/**
 * 唯一的查询入口。返回 null 表示响应已过期，调用方应当丢弃。
 *
 * 阶段 4 起 Rust 侧会返回 Err(String)（可直接展示给用户的中文提示）。
 * 这里接住并塞进 status —— 不接的话数据库出错时界面是空白，
 * 用户只会觉得「搜不出来」，看不到任何原因。
 */
async function fetchItems(): Promise<ClipboardItem[] | null> {
  const { query, types, favoriteOnly, sensitive } = useStore.getState();
  const seq = ++reqSeq;
  try {
    const items = await api.list({
      text: query,
      types,
      favoriteOnly,
      includeSensitive: sensitive,
      limit: PAGE_LIMIT,
    });
    return seq === reqSeq ? items : null;
  } catch (e) {
    if (seq === reqSeq) {
      useStore.getState().say(errorText(e), "err");
    }
    return null;
  }
}

function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}

/**
 * 跑一个会失败的后端调用，失败时把可读文案显示在状态栏。
 *
 * Rust 侧的错误已经翻成能直接展示的中文（"删除失败：数据库错误: ..."），
 * 所以优先用后端给的文案，只在拿不到时才用 fallback 兜底 ——
 * 宁可重复也不能丢信息。
 */
async function attempt(fallback: string, run: () => Promise<unknown>): Promise<boolean> {
  try {
    await run();
    return true;
  } catch (e) {
    const msg = e === undefined || e === null ? "" : errorText(e);
    useStore.getState().say(msg || fallback, "err");
    return false;
  }
}

export const useStore = create<State>((set, get) => ({
  items: [],
  actions: [],
  loading: false,
  query: "",
  types: [],
  favoriteOnly: false,
  sensitive: false,
  view: "list",
  settings: null,
  selected: 0,
  status: null,
  menu: null,

  async init() {
    set({ loading: true });
    // 后端抓到新内容会推 clipboard://changed。订阅一次就够 ——
    // init 只在挂载时调一次，React 严格模式下的重复调用也拿不到新结果，
    // 多订阅只会让同一条内容刷两遍
    api.subscribe(
      () => void useStore.getState().refresh(),
      // 降级提示直接显示在状态栏。降级不是错误 —— 内容确实复制
      // 成功了，只是没自动粘上，所以用 warn 语气而不是 err
      (text) => useStore.getState().say(text, "warn"),
    );
    try {
      await get().refresh();
    } finally {
      set({ loading: false });
    }
  },

  async refresh() {
    // 立即拉取意味着放弃还没触发的防抖，否则它会再打一次同样的查询
    clearTimeout(queryTimer);
    // 选中项必须在发请求前记下来：一是收藏会改变后端排序，
    // 二是等响应期间光标可能已经移走，那时再读就串行了
    const cur = get().selected;
    const id = get().items[cur]?.id;
    const items = await fetchItems();
    if (!items) return;
    // 按 id 找回同一项，免得光标底下的行突然换成另一条
    const at = id === undefined ? -1 : items.findIndex((x) => x.id === id);
    set({ items, selected: at >= 0 ? at : clamp(cur, items.length) });
  },

  async refilter() {
    clearTimeout(queryTimer);
    set({ selected: 0 });
    const items = await fetchItems();
    if (!items) return;
    set({ items, selected: 0 });
  },

  setQuery(query) {
    set({ query, selected: 0 });
    clearTimeout(queryTimer);
    queryTimer = setTimeout(() => void get().refilter(), SEARCH_DEBOUNCE);
  },

  toggleType(t) {
    const cur = get().types;
    set({ types: cur.includes(t) ? cur.filter((x) => x !== t) : [...cur, t] });
    void get().refilter();
  },

  toggleFavoriteOnly() {
    set({ favoriteOnly: !get().favoriteOnly });
    void get().refilter();
  },

  toggleSensitive() {
    set({ sensitive: !get().sensitive });
    void get().refilter();
  },

  clearFilters() {
    set({ types: [], favoriteOnly: false, sensitive: false });
    void get().refilter();
  },

  select(i) {
    set({ selected: i });
  },

  move(d) {
    const n = get().items.length;
    if (n === 0) return;
    // 首尾相接，对应原来 cmdk 的 loop
    set({ selected: (((get().selected + d) % n) + n) % n });
  },

  jump(i) {
    set({ selected: clamp(i, get().items.length) });
  },

  async loadActions(t) {
    const actions = await api.availableActions(t);
    // 快速移动选中项时，旧的慢响应会盖掉新的
    if (useStore.getState().items[useStore.getState().selected]?.contentType !== t) return;
    set({ actions });
  },

  // ── 设置页 ──────────────────────────────────────────────────
  openSettings() {
    set({ view: "settings", settings: null });
    api
      .getSettings()
      .then((s) => set({ settings: s }))
      .catch((e) => get().say(errorText(e) || "读取设置失败", "err"));
  },

  closeSettings() {
    set({ view: "list" });
  },

  async saveSettings(patch) {
    // 后端返回合并后的完整设置，直接拿来当新草稿 ——
    // 数值范围截断发生在后端，前端显示的应是被接受的那个值
    try {
      const s = await api.setSettings(patch);
      set({ settings: s });
      get().say("设置已保存");
    } catch (e) {
      get().say(errorText(e) || "保存设置失败", "err");
    }
  },

  async saveHotkey(accel) {
    try {
      const canon = await api.setHotkey(accel);
      set({ settings: { ...(get().settings as Settings), hotkey: canon } });
      get().say(`快捷键已改为 ${canon}`);
    } catch (e) {
      get().say(errorText(e) || "保存快捷键失败", "err");
    }
  },

  // 以下都是「操作 + 重新拉取」的形状。阶段 4 起 Rust 会返回
  // Err(String)，所以统一用 attempt 包一层：出错时把可读的
  // 提示显示在状态栏，而不是让 rejected promise 悄悄溜走
  async toggleFavorite(id) {
    if (!(await attempt("切换收藏失败", () => api.toggleFavorite(id)))) return;
    await get().refresh();
    get().say("已切换收藏");
  },

  async copy(id) {
    if (!(await attempt("复制失败", () => api.copyToClipboard(id)))) return;
    get().say("已复制到剪贴板");
  },

  async paste(id) {
    if (!(await attempt("粘贴失败", () => api.paste(id)))) return;
    await get().refresh();
    get().say("已粘贴");
  },

  async remove(id) {
    if (!(await attempt("删除失败", () => api.remove([id])))) return;
    await get().refresh();
    get().say("已删除");
  },

  /**
   * 跑一个工具箱动作。
   *
   * 两种失败要分开：动作本身算不出来时后端返回 `{ ok: false }`
   * （那是动作给的、能指导下一步的中文），而 invoke 整个失败
   * （条目没了、数据库锁着）才走 attempt 的兜底。
   *
   * 动作成功后**不刷新列表** —— 结果写的是系统剪贴板，
   * Rust 侧用 SelfWrite 挡掉了自己这一次写入，历史里不会多出新条目
   */
  async runAction(item, actionId) {
    let r: ActionResult;
    try {
      r = await api.runToolboxAction(item.id, actionId);
    } catch (e) {
      get().say(errorText(e) || "动作执行失败", "err");
      return;
    }
    get().say(r.ok ? r.value : r.error, r.ok ? "ok" : "err");
  },

  say(text, kind = "ok") {
    clearTimeout(statusTimer);
    set({ status: { text, kind } });
    statusTimer = setTimeout(() => set({ status: null }), 2600);
  },

  openMenu(item, x, y) {
    set({ menu: { item, x, y } });
  },

  closeMenu() {
    set({ menu: null });
  },
}));

export const backendLabel = backendName;
