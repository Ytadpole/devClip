import { useEffect, useState } from "react";
import { useStore } from "../store";
import type { Settings } from "../lib/api";

/**
 * 设置页。盖在调色板列表上，Esc 退回。
 *
 * 字段只放 docs/06 阶段 7 清单点名的五项。theme 虽在 Settings 契约里，
 * 但 UI 还没有亮色主题可切，放一个不生效的开关是骗人 —— 不放。
 *
 * 数值以文本框编辑、失焦不改，点「保存」才提交：快捷键那种
 * 打一半的状态不该被当成值发出去
 */
export function SettingsPanel() {
  const settings = useStore((s) => s.settings);
  const status = useStore((s) => s.status);
  const saveSettings = useStore((s) => s.saveSettings);
  const saveHotkey = useStore((s) => s.saveHotkey);
  const closeSettings = useStore((s) => s.closeSettings);

  // 本地草稿。settings 为 null 表示还在加载，面板先不出
  const [draft, setDraft] = useState<Settings | null>(null);
  const [hotkey, setHotkey] = useState("");

  useEffect(() => {
    if (settings) {
      setDraft(settings);
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
      className="overflow-hidden rounded-xl border border-white/10 bg-zinc-900/80 shadow-2xl shadow-black/60 backdrop-blur-xl"
    >
      <div className="flex items-center border-b border-white/10 px-4">
        <span className="h-14 flex-1 self-center text-[15px] text-zinc-100">设置</span>
        <button
          type="button"
          onClick={closeSettings}
          className="rounded px-1.5 text-xs text-zinc-600 hover:text-zinc-300"
        >
          返回（Esc）
        </button>
      </div>

      <div className="space-y-4 px-4 py-4 text-[13px]">
        <label className="block">
          <span className="mb-1 block text-zinc-400">全局快捷键</span>
          <div className="flex gap-2">
            <input
              value={hotkey}
              onChange={(e) => setHotkey(e.target.value)}
              spellCheck={false}
              placeholder="Alt+Shift+V"
              className="w-44 rounded-md border border-white/10 bg-white/[0.04] px-2 py-1 font-mono text-zinc-100 outline-none focus:border-sky-400/50"
            />
            <button
              type="button"
              onClick={() => void saveHotkey(hotkey.trim())}
              className="rounded-md border border-white/10 bg-white/[0.04] px-2.5 py-1 text-zinc-300 transition-colors hover:border-sky-400/40 hover:text-sky-200"
            >
              保存快捷键
            </button>
          </div>
          <span className="mt-1 block text-[11px] text-zinc-600">
            形如 Alt+Shift+V。被占用或系统未授权会在这里报错
          </span>
        </label>

        <label className="block">
          <span className="mb-1 block text-zinc-400">历史保留天数</span>
          <input
            type="number"
            min={0}
            max={3650}
            value={draft.retentionDays}
            onChange={(e) => setDraft({ ...draft, retentionDays: num(e.target.value, 0) })}
            className="w-32 rounded-md border border-white/10 bg-white/[0.04] px-2 py-1 text-zinc-100 outline-none focus:border-sky-400/50"
          />
          <span className="mt-1 block text-[11px] text-zinc-600">
            0 = 立即过期。收藏项永不自动清理
          </span>
        </label>

        <label className="block">
          <span className="mb-1 block text-zinc-400">历史最大条数</span>
          <input
            type="number"
            min={10}
            max={100000}
            value={draft.maxItems}
            onChange={(e) => setDraft({ ...draft, maxItems: num(e.target.value, 1000) })}
            className="w-32 rounded-md border border-white/10 bg-white/[0.04] px-2 py-1 text-zinc-100 outline-none focus:border-sky-400/50"
          />
        </label>

        <label className="block">
          <span className="mb-1 block text-zinc-400">图片大小上限（MB）</span>
          <input
            type="number"
            min={1}
            max={100}
            value={Math.max(1, Math.round(draft.maxImageBytes / 1048576))}
            onChange={(e) =>
              setDraft({ ...draft, maxImageBytes: num(e.target.value, 10) * 1048576 })
            }
            className="w-32 rounded-md border border-white/10 bg-white/[0.04] px-2 py-1 text-zinc-100 outline-none focus:border-sky-400/50"
          />
        </label>

        <label className="flex items-center gap-2">
          <input
            type="checkbox"
            checked={draft.sensitiveAutoExpire}
            onChange={(e) => setDraft({ ...draft, sensitiveAutoExpire: e.target.checked })}
            className="h-3.5 w-3.5 accent-sky-400"
          />
          <span className="text-zinc-300">
            敏感信息自动过期
            <span className="ml-2 text-[11px] text-zinc-600">
              疑似密钥的内容 60 秒后自动删除并清空剪贴板
            </span>
          </span>
        </label>
      </div>

      <div className="flex items-center border-t border-white/10 px-4 py-2 text-[11px]">
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
          className="rounded-md border border-sky-400/40 bg-sky-400/10 px-3 py-1 text-sky-200 transition-colors hover:bg-sky-400/20"
        >
          保存设置
        </button>
        <span className="ml-3 text-zinc-600">立即生效并持久化</span>
        {/* 状态栏在列表视图的底栏里，设置页得自己显示反馈 ——
            保存了却没有任何回音，用户只会再点一次 */}
        {status && (
          <span className={`ml-auto ${status.kind === "ok" ? "text-emerald-400" : "text-amber-400"}`}>
            {status.text}
          </span>
        )}
      </div>
    </div>
  );
}
