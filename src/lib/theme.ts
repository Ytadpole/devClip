/**
 * 主题 —— 只管把设置里的取值落成 <html> 上的 `data-theme`
 *
 * CSS 那边（index.css）默认深色，`data-theme="light"` 打开亮色，
 * 所以这里只做「取值 → 属性」这一件事，颜色一个字都不管。
 *
 * 刻意不用 prefers-color-scheme 直接驱动样式：主题是应用设置里的
 * 一项，「跟随系统」也走同一条路。否则用户选了深色、系统是亮色时
 * 就有两套来源互相打架，谁赢取决于 CSS 加载顺序
 */

import type { Theme } from "./api";

/** 真正落进 DOM 的两种。`system` 在这里现算，不原样传下去 */
export type Resolved = "dark" | "light";

const prefersDark = window.matchMedia("(prefers-color-scheme: dark)");

/** 跟随系统时按操作系统当前的设置算。只在变化时重算，别缓存 */
export function resolve(theme: Theme): Resolved {
  if (theme === "system") return prefersDark.matches ? "dark" : "light";
  return theme;
}

export function apply(theme: Theme): void {
  document.documentElement.dataset.theme = resolve(theme);
}

/**
 * 订阅系统主题变化。「跟随系统」得跟着变，只在切换那一刻取一次快照
 * 的话，用户从系统设置里改了主题，DevClip 要重启才跟上。
 *
 * 返回取消函数
 */
export function onSystemChange(cb: () => void): () => void {
  prefersDark.addEventListener("change", cb);
  return () => prefersDark.removeEventListener("change", cb);
}
