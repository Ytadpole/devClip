import { useEffect, useRef } from "react";
import { Command } from "cmdk";
import { ContextMenu } from "./ContextMenu";
import { SettingsPanel } from "./SettingsPanel";
import { TypeBadge } from "./TypeBadge";
import { VirtualList } from "./VirtualList";
import { backendLabel, useStore } from "../store";
import { hideWindow, onOpenSettings } from "../lib/backend";
import type { ContentType } from "../lib/api";

/** 筛选栏只展示高频类型，完整列表留给设置页 */
const QUICK_TYPES: ContentType[] = ["json", "sql", "code", "url", "jwt", "text"];

export function Palette() {
  const s = useStore();
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    void useStore.getState().init();
  }, []);

  // 托盘菜单点了「设置」。取消订阅跟着组件卸载走
  useEffect(() => onOpenSettings(() => useStore.getState().openSettings()), []);

  // 选中项的类型变了才重新拉工具箱动作。按 items 整个依赖会
  // 在任何一次列表刷新后都重拉一遍，没必要
  const activeType = s.items[s.selected]?.contentType;
  useEffect(() => {
    if (activeType) void useStore.getState().loadActions(activeType);
    else useStore.setState({ actions: [] });
  }, [activeType]);

  // 全局快捷键。
  //
  // ↑↓/Home/End 以及 cmdk 的 vim 键位在这里接管：cmdk 是靠在 DOM 里
  // 查已渲染的项来导航的（见 cmdk 的 k()/G()），而列表做了虚拟化，
  // 它只看得到窗口内的项，算出来的下一项是错的。捕获阶段 +
  // stopPropagation 就是为了让 cmdk 收不到这些按键。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const st = useStore.getState();
      const mod = e.metaKey || e.ctrlKey;
      const k = e.key.toLowerCase();

      // 设置页盖着列表时，导航键都是输入框的事，这里只管 Esc 退回
      if (st.view === "settings") {
        if (e.key === "Escape") {
          e.preventDefault();
          st.closeSettings();
        }
        return;
      }

      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        e.stopPropagation();
        st.move(e.key === "ArrowDown" ? 1 : -1);
        return;
      }
      if (!mod && (e.key === "Home" || e.key === "End")) {
        e.preventDefault();
        e.stopPropagation();
        st.jump(e.key === "Home" ? 0 : st.items.length - 1);
        return;
      }
      // cmdk 默认把 ⌃J/⌃N 当下一个、⌃K/⌃P 当上一个。上面已经接管，
      // 所以下面 Command 上也关了 vimBindings —— 万一拦截没生效，
      // 也不能让 cmdk 再按「窗口内导航」那套错逻辑动一次
      if (e.ctrlKey && !e.altKey && !e.metaKey && (k === "j" || k === "n" || k === "k" || k === "p")) {
        e.preventDefault();
        e.stopPropagation();
        st.move(k === "j" || k === "n" ? 1 : -1);
        return;
      }

      if (mod && k === "k") {
        e.preventDefault();
        inputRef.current?.focus();
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        // 三级逐层退出：菜单 → 搜索词 → 收起面板。
        // 最后一级的「收起」在浏览器里是空操作，e2e 跑的就是那条路
        if (st.menu) st.closeMenu();
        else if (st.query) st.setQuery("");
        else void hideWindow();
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

    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  const selected = s.items[s.selected];
  const filtered = s.types.length > 0 || s.favoriteOnly || s.sensitive;

  return (
    <div className="flex min-h-screen justify-center pt-[11vh] text-fg antialiased">
      <div className="w-full max-w-[680px] px-4">
        {s.view === "settings" ? (
          <SettingsPanel />
        ) : (
        <Command
          label="DevClip 剪贴板"
          shouldFilter={false}
          vimBindings={false}
          value={selected ? String(selected.id) : ""}
          onValueChange={(v) => {
            const i = s.items.findIndex((x) => String(x.id) === v);
            if (i >= 0) useStore.getState().select(i);
          }}
          data-panel-root
          className="overflow-hidden rounded-xl bg-panel/95 shadow-2xl shadow-black/60 backdrop-blur-xl light:shadow-black/10"
        >
          {/* 搜索框 */}
          <div className="flex items-center gap-3 border-b border-line px-4">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-4 w-4 shrink-0 text-faint">
              <circle cx="11" cy="11" r="7" />
              <path strokeLinecap="round" d="M20 20l-3.5-3.5" />
            </svg>
            <Command.Input
              ref={inputRef}
              value={s.query}
              onValueChange={s.setQuery}
              autoFocus
              placeholder="搜索历史记录…（⌘K 聚焦）"
              className="h-14 flex-1 bg-transparent text-[15px] text-fg-strong outline-none placeholder:text-faint"
            />
            {s.query && (
              <button
                type="button"
                onClick={() => s.setQuery("")}
                className="rounded px-1.5 text-xs text-faint transition-colors hover:text-fg"
              >
                清除
              </button>
            )}
          </div>

          {/* 类型筛选 */}
          <div className="flex flex-wrap items-center gap-1.5 border-b border-line-soft px-3 py-2">
            <button
              type="button"
              onClick={s.toggleFavoriteOnly}
              className={`rounded-md px-2 py-0.5 text-[11px] transition-colors ${
                s.favoriteOnly
                  ? "bg-amber-400/20 text-amber-300 light:text-amber-700"
                  : "text-muted hover:bg-hover hover:text-fg"
              }`}
            >
              ⭐ 仅收藏
            </button>
            <button
              type="button"
              onClick={s.toggleSensitive}
              title="展开疑似密钥 / token 的分组"
              className={`rounded-md px-2 py-0.5 text-[11px] transition-colors ${
                s.sensitive
                  ? "bg-rose-400/20 text-rose-300 light:text-rose-700"
                  : "text-muted hover:bg-hover hover:text-fg"
              }`}
            >
              🔒 含敏感
            </button>
            <span className="mx-0.5 h-3.5 w-px bg-line" />
            {QUICK_TYPES.map((t) => (
              <button
                key={t}
                type="button"
                onClick={() => s.toggleType(t)}
                className={`rounded-md px-2 py-0.5 text-[11px] transition-colors ${
                  s.types.includes(t)
                    ? "bg-sky-400/20 text-sky-300 light:text-sky-700"
                    : "text-muted hover:bg-hover hover:text-fg"
                }`}
              >
                {t}
              </button>
            ))}
            <span className="ml-auto flex items-center gap-2 text-[11px] text-faint">
              {filtered && (
                <button
                  type="button"
                  onClick={s.clearFilters}
                  className="rounded px-1.5 py-0.5 transition-colors hover:bg-hover hover:text-fg"
                >
                  清除筛选
                </button>
              )}
              <button
                type="button"
                onClick={s.openSettings}
                title="设置"
                className="rounded px-1.5 py-0.5 transition-colors hover:bg-hover hover:text-fg"
              >
                ⚙ 设置
              </button>
              共 {s.items.length} 条
            </span>
          </div>

          {/* 列表（虚拟化，见 VirtualList） */}
          <VirtualList />

          {/* 工具箱动作条。列表来自 availableActions()，
              前端不认具体类型（见 AGENTS.md 的两咽喉点）。
              data-toolbox 是给 e2e 用的稳定钩子 —— 靠按钮文字
              找元素的话，改一次文案就得改一次测试 */}
          {selected && s.actions.length > 0 && (
            <div
              data-toolbox=""
              className="flex items-center gap-1.5 border-t border-line px-3 py-2"
            >
              <span className="text-[11px] text-faint">工具箱</span>
              {s.actions.map((a) => (
                <button
                  key={a.id}
                  type="button"
                  // title 而不是行内文字：按钮条已经贴着窗口边，
                  // 再塞说明会把状态栏挤掉。悬停能看到就够 ——
                  // 而 JWT「base64 不是加密」这类必须看到的话，
                  // 悬停是唯一不破坏布局的位置
                  title={a.hint}
                  onClick={() => void s.runAction(selected, a.id)}
                  className="rounded-md border border-line bg-field px-2 py-0.5 text-[11px] text-fg transition-colors hover:border-sky-400/40 hover:bg-sky-400/10 hover:text-sky-200 light:hover:text-sky-700"
                >
                  {a.label}
                </button>
              ))}
              <span className="ml-auto text-[11px] text-faint">结果写入剪贴板</span>
            </div>
          )}

          {/* 底部：状态 / 快捷键提示 */}
          <div className="flex items-center gap-3 border-t border-line px-3 py-2 text-[11px] text-faint">
            {s.status ? (
              <span
                data-status={s.status.kind}
                className={
                  s.status.kind === "ok"
                    ? "text-emerald-400 light:text-emerald-700"
                    : // warn 与 err 同色但语义不同：降级不是故障，
                      // 统一用琥珀色表示「没按预期走」，不制造恐慌
                      "text-amber-400 light:text-amber-700"
                }
              >
                {s.status.text}
              </span>
            ) : (
              <>
                <span>↑↓ 选择</span>
                <span>↵ 粘贴</span>
                <span>⌘C 复制</span>
                <span>⌘D 收藏</span>
                <span>右键 更多</span>
                <span className="ml-auto flex items-center gap-2">
                  {selected && <TypeBadge type={selected.contentType} />}
                  <span className="rounded bg-field px-1.5 py-0.5 text-muted">
                    {backendLabel} 后端
                  </span>
                </span>
              </>
            )}
          </div>
        </Command>
        )}
      </div>

      <ContextMenu />
    </div>
  );
}
