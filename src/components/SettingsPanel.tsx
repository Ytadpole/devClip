import { useEffect, useState } from "react";
import { useStore } from "../store";
import type { Settings, Theme } from "../lib/api";

/** 走「保存设置」按钮提交的字段。快捷键与主题另有各的通道 */
type Draft = Pick<Settings, "retentionDays" | "maxItems" | "maxImageBytes" | "sensitiveAutoExpire">;

/**
 * 设置页。盖在面板列表上，Esc 退回。
 *
 * 字段只放 docs/06 阶段 7 清单点名的那些，外加主题。
 *
 * 数值以文本框编辑、失焦不改，点「保存」才提交：快捷键那种
 * 打一半的状态不该被当成值发出去。**主题不在草稿里** ——
 * 它是「点一下就看见」的那一类，跟着草稿提交会让用户以为没生效
 */
export function SettingsPanel() {
  const settings = useStore((s) => s.settings);
  const status = useStore((s) => s.status);
  const theme = useStore((s) => s.theme);
  const saveSettings = useStore((s) => s.saveSettings);
  const saveHotkey = useStore((s) => s.saveHotkey);
  const setTheme = useStore((s) => s.setTheme);
  const closeSettings = useStore((s) => s.closeSettings);

  // 本地草稿。settings 为 null 表示还在加载，面板先不出。
  // 刻意不含 hotkey（另有 state）与 theme（另有 state）：这两项
  // 不经过「保存设置」，混进草稿只会让人以为改了会一起提交
  const [draft, setDraft] = useState<Draft | null>(null);
  const [hotkey, setHotkey] = useState("");

  useEffect(() => {
    if (settings) {
      setDraft({
        retentionDays: settings.retentionDays,
        maxItems: settings.maxItems,
        maxImageBytes: settings.maxImageBytes,
        sensitiveAutoExpire: settings.sensitiveAutoExpire,
      });
      setHotkey(settings.hotkey);
    }
  }, [settings]);

  if (!draft) return null;

  const num = (v: string, fallback: number) => {
    const n = Number(v);
    return Number.isFinite(n) ? n : fallback;
  };

  return (
    <div
      data-settings=""
      data-panel-root
      className="overflow-hidden rounded-xl bg-panel/95 shadow-2xl shadow-black/60 backdrop-blur-xl light:shadow-black/10"
    >
      <div className="flex items-center border-b border-line px-4">
        <span className="h-14 flex-1 self-center text-[15px] text-fg-strong">设置</span>
        <button
          type="button"
          onClick={closeSettings}
          className="rounded px-1.5 text-xs text-faint transition-colors hover:text-fg"
        >
          返回（Esc）
        </button>
      </div>

      <div className="space-y-4 px-4 py-4 text-[13px]">
        <div>
          <span className="mb-1 block text-label">主题</span>
          <ThemePicker theme={theme} onPick={(t) => void setTheme(t)} />
          <span className="mt-1 block text-[11px] text-faint">
            立即生效并持久化。「跟随系统」在系统设置里改主题也跟着变
          </span>
        </div>

        <label className="block">
          <span className="mb-1 block text-label">全局快捷键</span>
          <div className="flex gap-2">
            <input
              value={hotkey}
              onChange={(e) => setHotkey(e.target.value)}
              spellCheck={false}
              placeholder="Alt+Shift+V"
              className="w-44 rounded-md border border-line bg-field px-2 py-1 font-mono text-fg-strong outline-none focus:border-sky-400/50"
            />
            <button
              type="button"
              onClick={() => void saveHotkey(hotkey.trim())}
              className="rounded-md border border-line bg-field px-2.5 py-1 text-fg transition-colors hover:border-sky-400/40 hover:bg-sky-400/10 hover:text-sky-200 light:hover:text-sky-700"
            >
              保存快捷键
            </button>
          </div>
          <span className="mt-1 block text-[11px] text-faint">
            形如 Alt+Shift+V。被占用或系统未授权会在这里报错
          </span>
        </label>

        <label className="block">
          <span className="mb-1 block text-label">历史保留天数</span>
          <input
            type="number"
            min={0}
            max={3650}
            value={draft.retentionDays}
            onChange={(e) => setDraft({ ...draft, retentionDays: num(e.target.value, 0) })}
            className="w-32 rounded-md border border-line bg-field px-2 py-1 text-fg-strong outline-none focus:border-sky-400/50"
          />
          <span className="mt-1 block text-[11px] text-faint">
            0 = 立即过期。收藏项永不自动清理
          </span>
        </label>

        <label className="block">
          <span className="mb-1 block text-label">历史最大条数</span>
          <input
            type="number"
            min={10}
            max={100000}
            value={draft.maxItems}
            onChange={(e) => setDraft({ ...draft, maxItems: num(e.target.value, 1000) })}
            className="w-32 rounded-md border border-line bg-field px-2 py-1 text-fg-strong outline-none focus:border-sky-400/50"
          />
        </label>

        <label className="block">
          <span className="mb-1 block text-label">图片大小上限（MB）</span>
          <input
            type="number"
            min={1}
            max={100}
            value={Math.max(1, Math.round(draft.maxImageBytes / 1048576))}
            onChange={(e) =>
              setDraft({ ...draft, maxImageBytes: num(e.target.value, 10) * 1048576 })
            }
            className="w-32 rounded-md border border-line bg-field px-2 py-1 text-fg-strong outline-none focus:border-sky-400/50"
          />
        </label>

        <label className="flex items-center gap-2">
          <input
            type="checkbox"
            checked={draft.sensitiveAutoExpire}
            onChange={(e) => setDraft({ ...draft, sensitiveAutoExpire: e.target.checked })}
            className="h-3.5 w-3.5 accent-sky-400"
          />
          <span className="text-fg">
            敏感信息自动过期
            <span className="ml-2 text-[11px] text-faint">
              疑似密钥的内容 60 秒后自动删除并清空剪贴板
            </span>
          </span>
        </label>
      </div>

      <div className="flex items-center border-t border-line px-4 py-2 text-[11px]">
        <button
          type="button"
          onClick={() =>
            void saveSettings({
              retentionDays: draft.retentionDays,
              maxItems: draft.maxItems,
              maxImageBytes: draft.maxImageBytes,
              sensitiveAutoExpire: draft.sensitiveAutoExpire,
            })
          }
          className="rounded-md border border-sky-400/40 bg-sky-400/10 px-3 py-1 text-sky-200 transition-colors hover:bg-sky-400/20 light:border-sky-700/40 light:bg-sky-700/10 light:text-sky-700 light:hover:bg-sky-700/20"
        >
          保存设置
        </button>
        <span className="ml-3 text-faint">立即生效并持久化</span>
        {/* 状态栏在列表视图的底栏里，设置页得自己显示反馈 ——
            保存了却没有任何回音，用户只会再点一次 */}
        {status && (
          <span
            data-status={status.kind}
            className={`ml-auto ${
              status.kind === "ok" ? "text-emerald-400 light:text-emerald-700" : "text-amber-400 light:text-amber-700"
            }`}
          >
            {status.text}
          </span>
        )}
      </div>
    </div>
  );
}

/** 三个取值的单选。data-theme-picker 是给 e2e 用的稳定钩子 */
const THEME_OPTIONS: Array<{ value: Theme; label: string }> = [
  { value: "system", label: "跟随系统" },
  { value: "dark", label: "深色" },
  { value: "light", label: "亮色" },
];

function ThemePicker({ theme, onPick }: { theme: Theme; onPick: (t: Theme) => void }) {
  return (
    <div data-theme-picker="" className="flex gap-1.5">
      {THEME_OPTIONS.map((o) => (
        <button
          key={o.value}
          type="button"
          // aria-pressed 而不是 radio：三选一的按钮组读屏播报
          // 「深色 已按下」比「单选按钮 3 之 2」更好懂
          aria-pressed={theme === o.value}
          onClick={() => onPick(o.value)}
          className={`rounded-md border px-2.5 py-1 text-[12px] transition-colors ${
            theme === o.value
              ? "border-line bg-active text-fg-strong"
              : "text-faint hover:bg-hover hover:text-fg"
          }`}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}
