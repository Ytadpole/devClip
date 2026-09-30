import type { ContentType } from "./api";

/** 相对时间：列表里显示「2 分钟前」这类 */
export function relTime(ts: number, now = Date.now()): string {
  const m = Math.floor(Math.max(0, now - ts) / 60_000);
  if (m < 1) return "刚刚";
  if (m < 60) return `${m} 分钟前`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h} 小时前`;
  const d = Math.floor(h / 24);
  if (d < 30) return `${d} 天前`;
  return new Date(ts).toLocaleDateString("zh-CN");
}

export function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

interface TypeMeta {
  label: string;
  /** 徽标配色 */
  badge: string;
  /** 左侧色条 */
  bar: string;
}

/**
 * 每种内容类型的展示元数据。阶段 1 前端持有；阶段 3 后由 Rust 侧 detect 负责判定，前端只管显示。
 *
 * 强调色不在 index.css 的 @theme 里：那套变量是全局的，一个颜色一个变量，
 * 13 种类型再乘两套主题就是 26 个变量，而这份表本来就得在每个类型旁边
 * 同时写清深浅两色（见 AGENTS.md 的主题约定）。
 *
 * `light:` 变体只有徽标文字和色条需要：`bg-*-400/15` 那个底色在白底上
 * 淡到快看不见，但作为「这里有个徽标」的提示反而正好，不值得再调
 */
export const TYPE_META: Record<ContentType, TypeMeta> = {
  json: { label: "JSON", badge: "bg-amber-400/15 text-amber-300 light:text-amber-700", bar: "bg-amber-400 light:bg-amber-700" },
  sql: { label: "SQL", badge: "bg-sky-400/15 text-sky-300 light:text-sky-700", bar: "bg-sky-400 light:bg-sky-700" },
  jwt: { label: "JWT", badge: "bg-violet-400/15 text-violet-300 light:text-violet-700", bar: "bg-violet-400 light:bg-violet-700" },
  code: { label: "Code", badge: "bg-emerald-400/15 text-emerald-300 light:text-emerald-700", bar: "bg-emerald-400 light:bg-emerald-700" },
  url: { label: "URL", badge: "bg-blue-400/15 text-blue-300 light:text-blue-700", bar: "bg-blue-400 light:bg-blue-700" },
  base64: { label: "Base64", badge: "bg-teal-400/15 text-teal-300 light:text-teal-700", bar: "bg-teal-400 light:bg-teal-700" },
  uuid: { label: "UUID", badge: "bg-fuchsia-400/15 text-fuchsia-300 light:text-fuchsia-700", bar: "bg-fuchsia-400 light:bg-fuchsia-700" },
  ip: { label: "IP", badge: "bg-orange-400/15 text-orange-300 light:text-orange-700", bar: "bg-orange-400 light:bg-orange-700" },
  commit: { label: "Commit", badge: "bg-lime-400/15 text-lime-300 light:text-lime-700", bar: "bg-lime-400 light:bg-lime-700" },
  markdown: { label: "MD", badge: "bg-indigo-400/15 text-indigo-300 light:text-indigo-700", bar: "bg-indigo-400 light:bg-indigo-700" },
  exception: { label: "Exception", badge: "bg-rose-400/15 text-rose-300 light:text-rose-700", bar: "bg-rose-400 light:bg-rose-700" },
  image: { label: "Image", badge: "bg-pink-400/15 text-pink-300 light:text-pink-700", bar: "bg-pink-400 light:bg-pink-700" },
  // 中性色不跟上面一档：深色用 zinc-400，浅色得再深一档（zinc-500）才够读
  text: { label: "Text", badge: "bg-zinc-400/15 text-zinc-400 light:text-zinc-500", bar: "bg-zinc-500 light:bg-zinc-600" },
};

/** 单行预览：压掉换行和多余空白 */
export function oneLine(s: string, max = 160): string {
  const t = s.replace(/\s+/g, " ").trim();
  return t.length > max ? t.slice(0, max) + "…" : t;
}
