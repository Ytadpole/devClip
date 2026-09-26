import { useEffect, useRef } from "react";
import { Command } from "cmdk";
import { ItemRow } from "./ItemRow";
import { TypeBadge } from "./TypeBadge";
import { ALL_TYPES, backendLabel, useStore } from "../store";
import type { ContentType } from "../lib/api";

/** 筛选栏只展示高频类型，完整列表在设置里 */
const QUICK_TYPES: ContentType[] = ["json", "sql", "code", "url", "jwt", "text"];

export function Palette() {
  const s = useStore();
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    void useStore.getState().init();
  }, []);

  // 选中项变化时，拉取它可用的工具箱动作
  useEffect(() => {
    const it = s.items[s.selected];
    if (it) void useStore.getState().loadActions(it);
    else useStore.setState({ actions: [] });
  }, [s.items, s.selected]);

  // 全局快捷键（输入框之外的组合键）
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const st = useStore.getState();
      const mod = e.metaKey || e.ctrlKey;
      const k = e.key.toLowerCase();

      if (mod && k === "k") {
        e.preventDefault();
        inputRef.current?.focus();
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        if (st.query) st.setQuery("");
        else inputRef.current?.blur();
        return;
      }

      const sel = st.items[st.selected];
      if (!sel) return;

      if (mod && k === "c") {
        e.preventDefault();
        void st.copy(sel.id);
      } else if (mod && k === "d") {
        e.preventDefault();
        void st.toggleFavorite(sel.id);
      } else if (mod && e.key === "Backspace") {
        e.preventDefault();
        void st.remove(sel.id);
      }
    };

    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const selected = s.items[s.selected];

  return (
    <div className="flex min-h-screen justify-center bg-zinc-950 pt-[11vh] text-zinc-300 antialiased">
      <div className="w-full max-w-[680px] px-4">
        <Command
          label="DevClip 剪贴板"
          shouldFilter={false}
          loop
          value={selected ? String(selected.id) : ""}
          onValueChange={(v) => {
            const i = s.items.findIndex((x) => String(x.id) === v);
            if (i >= 0) useStore.getState().select(i);
          }}
          className="overflow-hidden rounded-xl border border-white/10 bg-zinc-900/80 shadow-2xl shadow-black/60 backdrop-blur-xl"
        >
          {/* 搜索框 */}
          <div className="flex items-center gap-3 border-b border-white/10 px-4">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-4 w-4 shrink-0 text-zinc-600">
              <circle cx="11" cy="11" r="7" />
              <path strokeLinecap="round" d="M20 20l-3.5-3.5" />
            </svg>
            <Command.Input
              ref={inputRef}
              value={s.query}
              onValueChange={s.setQuery}
              autoFocus
              placeholder="搜索历史记录…（⌘K 聚焦）"
              className="h-14 flex-1 bg-transparent text-[15px] text-zinc-100 outline-none placeholder:text-zinc-600"
            />
            {s.query && (
              <button
                type="button"
                onClick={() => s.setQuery("")}
                className="rounded px-1.5 text-xs text-zinc-600 hover:text-zinc-300"
              >
                清除
              </button>
            )}
          </div>

          {/* 类型筛选 */}
          <div className="flex flex-wrap items-center gap-1.5 border-b border-white/5 px-3 py-2">
            <button
              type="button"
              onClick={s.toggleFavoriteOnly}
              className={`rounded-md px-2 py-0.5 text-[11px] transition-colors ${
                s.favoriteOnly
                  ? "bg-amber-400/20 text-amber-300"
                  : "text-zinc-500 hover:bg-white/5 hover:text-zinc-300"
              }`}
            >
              ⭐ 仅收藏
            </button>
            <span className="mx-0.5 h-3.5 w-px bg-white/10" />
            {QUICK_TYPES.map((t) => (
              <button
                key={t}
                type="button"
                onClick={() => s.toggleType(t)}
                className={`rounded-md px-2 py-0.5 text-[11px] transition-colors ${
                  s.types.includes(t)
                    ? "bg-sky-400/20 text-sky-300"
                    : "text-zinc-500 hover:bg-white/5 hover:text-zinc-300"
                }`}
              >
                {t}
              </button>
            ))}
            {s.types.length > 0 && (
              <button
                type="button"
                onClick={() => ALL_TYPES.forEach(() => {})}
                className="ml-auto text-[11px] text-zinc-600 hover:text-zinc-400"
              >
                共 {s.items.length} 条
              </button>
            )}
          </div>

          {/* 列表 */}
          <Command.List className="max-h-[min(420px,50vh)] overflow-y-auto overscroll-contain p-1.5">
            {s.loading ? (
              <div className="px-3 py-10 text-center text-sm text-zinc-600">加载中…</div>
            ) : (
              <>
                <Command.Empty className="px-3 py-10 text-center text-sm text-zinc-600">
                  没有匹配「{s.query}」的记录
                </Command.Empty>
                {s.items.map((it, i) => (
                  <ItemRow
                    key={it.id}
                    item={it}
                    active={i === s.selected}
                    onSelect={(id) => void s.paste(id)}
                    onToggleFavorite={(id) => void s.toggleFavorite(id)}
                  />
                ))}
              </>
            )}
          </Command.List>

          {/* 工具箱动作条 */}
          {selected && s.actions.length > 0 && (
            <div className="flex items-center gap-1.5 border-t border-white/10 px-3 py-2">
              <span className="text-[11px] text-zinc-600">工具箱</span>
              {s.actions.map((a) => (
                <button
                  key={a.id}
                  type="button"
                  onClick={() => void s.runAction(selected, a.id)}
                  className="rounded-md border border-white/10 bg-white/[0.04] px-2 py-0.5 text-[11px] text-zinc-300 transition-colors hover:border-sky-400/40 hover:bg-sky-400/10 hover:text-sky-200"
                >
                  {a.label}
                </button>
              ))}
            </div>
          )}

          {/* 底部：状态 / 快捷键提示 */}
          <div className="flex items-center gap-3 border-t border-white/10 px-3 py-2 text-[11px] text-zinc-600">
            {s.status ? (
              <span className={s.status.kind === "err" ? "text-amber-400" : "text-emerald-400"}>
                {s.status.text}
              </span>
            ) : (
              <>
                <span>↑↓ 选择</span>
                <span>↵ 粘贴</span>
                <span>⌘C 复制</span>
                <span>⌘D 收藏</span>
                <span className="ml-auto flex items-center gap-2">
                  {selected && <TypeBadge type={selected.contentType} />}
                  <span className="rounded bg-white/5 px-1.5 py-0.5 text-zinc-500">
                    {backendLabel} 后端 · 阶段 1
                  </span>
                </span>
              </>
            )}
          </div>
        </Command>
      </div>
    </div>
  );
}
