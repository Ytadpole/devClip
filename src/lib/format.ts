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

/** 每种内容类型的展示元数据。阶段 1 前端持有；阶段 3 后由 Rust 侧 detect 负责判定，前端只管显示。 */
export const TYPE_META: Record<ContentType, TypeMeta> = {
  json: { label: "JSON", badge: "bg-amber-400/15 text-amber-300", bar: "bg-amber-400" },
  sql: { label: "SQL", badge: "bg-sky-400/15 text-sky-300", bar: "bg-sky-400" },
  jwt: { label: "JWT", badge: "bg-violet-400/15 text-violet-300", bar: "bg-violet-400" },
  code: { label: "Code", badge: "bg-emerald-400/15 text-emerald-300", bar: "bg-emerald-400" },
  url: { label: "URL", badge: "bg-blue-400/15 text-blue-300", bar: "bg-blue-400" },
  base64: { label: "Base64", badge: "bg-teal-400/15 text-teal-300", bar: "bg-teal-400" },
  uuid: { label: "UUID", badge: "bg-fuchsia-400/15 text-fuchsia-300", bar: "bg-fuchsia-400" },
  ip: { label: "IP", badge: "bg-orange-400/15 text-orange-300", bar: "bg-orange-400" },
  commit: { label: "Commit", badge: "bg-lime-400/15 text-lime-300", bar: "bg-lime-400" },
  markdown: { label: "MD", badge: "bg-indigo-400/15 text-indigo-300", bar: "bg-indigo-400" },
  exception: { label: "Exception", badge: "bg-rose-400/15 text-rose-300", bar: "bg-rose-400" },
  image: { label: "Image", badge: "bg-pink-400/15 text-pink-300", bar: "bg-pink-400" },
  text: { label: "Text", badge: "bg-zinc-400/15 text-zinc-400", bar: "bg-zinc-500" },
};

/** 单行预览：压掉换行和多余空白 */
export function oneLine(s: string, max = 160): string {
  const t = s.replace(/\s+/g, " ").trim();
  return t.length > max ? t.slice(0, max) + "…" : t;
}
