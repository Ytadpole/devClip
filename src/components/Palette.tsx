import { useEffect, useRef } from "react";
import { Command } from "cmdk";
import { ContextMenu } from "./ContextMenu";
import { SettingsPanel } from "./SettingsPanel";
import { TypeBadge } from "./TypeBadge";
import { VirtualList } from "./VirtualList";
import { backendLabel, useStore } from "../store";
import { hideWindow, onOpenSettings, startWindowDrag } from "../lib/backend";
import { resolve } from "../lib/theme";
import type { ContentType } from "../lib/api";

/** 筛选栏只展示高频类型，完整列表留给设置页 */
const QUICK_TYPES: ContentType[] = ["json", "sql", "code", "url", "jwt", "text"];

export function Palette() {
  const s = useStore();
  const inputRef = useRef<HTMLInputElement>(null);

  // 搜索框兼做拖动热区：按下后移动超过阈值即进入窗口拖动，
  // 原地点击（未超阈值）照常聚焦打字。阈值期间监听挂在 window 上，
  // 鼠标移出输入框也能继续判定
  const inputDrag = useRef<{ x: number; y: number } | null>(null);
  const onInputMouseDown = (e: React.MouseEvent) => {
    inputDrag.current = { x: e.screenX, y: e.screenY };
    const onMove = (m: MouseEvent) => {
      if (!inputDrag.current) return;
      if (
        Math.abs(m.screenX - inputDrag.current.x) > 5 ||
        Math.abs(m.screenY - inputDrag.current.y) > 5
      ) {
        inputDrag.current = null;
        cleanup();
        // 拖动由 WM 接管指针，这行是自绘兜底；超时还原避免异常路径
        // 下光标卡死
        document.documentElement.style.cursor = "grabbing";
        setTimeout(() => {
          document.documentElement.style.cursor = "";
        }, 1000);
        startWindowDrag();
      }
    };
    const onUp = () => {
      inputDrag.current = null;
      cleanup();
    };
    const cleanup = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

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
    <div className="flex h-screen text-fg antialiased">
      <div className="h-full w-full">
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
          className="flex h-full flex-col overflow-hidden rounded-xl bg-panel/95 shadow-2xl shadow-black/60 backdrop-blur-xl light:shadow-black/10"
        >
          {/* 搜索行兼做拖拽区：按住空白处可移动面板，输入框点击不受影响 */}
          <div data-tauri-drag-region className="flex items-center gap-3 border-b border-line px-4">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-4 w-4 shrink-0 text-faint">
              <circle cx="11" cy="11" r="7" />
              <path strokeLinecap="round" d="M20 20l-3.5-3.5" />
            </svg>
            <Command.Input
              ref={inputRef}
              value={s.query}
              onValueChange={s.setQuery}
              onMouseDown={onInputMouseDown}
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
            {/* 换主题：不想为了切个颜色先退进设置页。图标画的是
                「点了会变成什么样」—— 现在亮色就画月亮（点了变暗），
                反之亦然，比反过来画好读
                data-theme-toggle 给 e2e 当稳定钩子，别改成靠
                aria-label 找：文案会改，钩子不会 */}
            <ThemeToggle />
            {/* 可见的拖动把手：输入框要留给打字，拖面板认这个把手
                和行内空白 */}
            <span
              data-tauri-drag-region
              title="拖动移动面板"
              className="cursor-grab select-none px-1 text-base leading-none text-faint hover:text-fg active:cursor-grabbing"
            >
              ⠿
            </span>
          </div>

          {/* 类型筛选。整行也是拖拽区（chip 与按钮是子元素不受影响） */}
          <div data-tauri-drag-region className="flex flex-wrap items-center gap-1.5 border-b border-line-soft px-3 py-2">
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
              <span className="ml-auto text-[11px] text-faint">变换后直接粘贴</span>
            </div>
          )}

          {/* 底部：状态 / 快捷键提示。同为拖拽区 */}
          <div data-tauri-drag-region className="flex items-center gap-3 border-t border-line px-3 py-2 text-[11px] text-faint">
            {s.monitorIssue ? (
              // 监听没起来时占掉整行。它不会自己好，藏在会消失的
              // status 里等于没说 —— 用户只会以为「复制了但历史里没有」
              <span data-status="warn" data-monitor-issue="" className="text-amber-400 light:text-amber-700">
                剪贴板监听未启用：{s.monitorIssue}
              </span>
            ) : s.status ? (
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

/**
 * 搜索行右侧的换主题按钮
 *
 * 只做「深色 ⇄ 亮色」两档。三档里的「跟随系统」仍然只在设置页里
 * 选——从面板上点一下就丢掉「跟随系统」这个意图太突兀，宁可让用户
 * 想要自动跟随时明确去设置页选一次。
 *
 * 翻转的基准是 `resolve()` 出来的**当前实际外观**，不是设置里存的
 * 原始取值：设置是「跟随系统」而系统此刻是亮色时，点一下应该得到
 * 深色（用户看到的是「我要另一个样子」），而不是把 system 原样
 * 翻成某个固定值。
 */
function ThemeToggle() {
  const theme = useStore((s) => s.theme);
  const setTheme = useStore((s) => s.setTheme);
  const now = resolve(theme);
  const next = now === "dark" ? "light" : "dark";

  return (
    <button
      type="button"
      data-theme-toggle=""
      onClick={() => void setTheme(next)}
      aria-label={next === "dark" ? "切换到深色" : "切换到亮色"}
      title={next === "dark" ? "切换到深色" : "切换到亮色"}
      className="shrink-0 rounded p-1 text-faint transition-colors hover:bg-hover hover:text-fg"
    >
      {now === "dark" ? (
        // 现在是深色，画月亮：点一下变暗色系之外的那一档
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          className="h-4 w-4"
          aria-hidden="true"
        >
          <path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z" />
        </svg>
      ) : (
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          className="h-4 w-4"
          aria-hidden="true"
        >
          <circle cx="12" cy="12" r="4" />
          <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
        </svg>
      )}
    </button>
  );
}
