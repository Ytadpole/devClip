import { useEffect, useRef, useState } from "react";
import { TypeBadge } from "./TypeBadge";
import { bytes } from "../lib/format";
import { useStore } from "../store";

/**
 * 内置编辑器（docs/04 通用操作 Edit）。
 *
 * 盖在列表上（与设置页同层），只编辑 content：类型识别与敏感扫描
 * 都由后端对新内容重跑（saveEdit 走 api.updateItem），前端不猜。
 *
 * Esc 一律取消，不问「有未保存的修改」—— 调色板是快进快出的工具，
 * 多一步确认比丢几个字的代价更常发生；保存是显式的 ⌘↵ / 按钮。
 * 失败时编辑器保持打开（saveEdit 返回 false），错误在状态栏。
 *
 * data-editor 与 data-editor-save 是 e2e 的稳定钩子，别改成靠
 * 按钮文字找：文案会改，钩子不会
 */
export function ItemEditor() {
  const editing = useStore((s) => s.editing)!;
  const closeEditor = useStore((s) => s.closeEditor);
  const saveEdit = useStore((s) => s.saveEdit);
  const status = useStore((s) => s.status);
  const [text, setText] = useState(() => editing.content);
  const ref = useRef<HTMLTextAreaElement>(null);

  // 光标落在末尾而不是全选：编辑多半是修几个字，全选一按就把
  // 原文覆盖了。textarea 挂 autoFocus 时 React 会把光标放回值末尾，
  // 但显式声明一次不依赖这个实现细节
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.focus();
    el.setSelectionRange(el.value.length, el.value.length);
  }, []);

  // 空内容等价于「把这条历史抹成一片空白」，后端也会拒 ——
  // 前端直接禁用保存，让按钮自己解释原因
  const empty = !text.trim();
  const save = () => {
    if (!empty) void saveEdit(editing.id, text);
  };

  return (
    <div data-editor="" className="flex h-full flex-col overflow-hidden rounded-xl border border-line bg-panel/95 backdrop-blur-xl">
      <div className="flex items-center gap-2 border-b border-line px-4 py-2.5 text-[12px] text-muted">
        <TypeBadge type={editing.contentType} />
        <span>编辑条目</span>
        <span className="ml-auto text-faint">{bytes(new TextEncoder().encode(text).length)}</span>
      </div>

      <textarea
        ref={ref}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
            e.preventDefault();
            save();
          }
        }}
        spellCheck={false}
        aria-label="编辑内容"
        className="flex-1 resize-none bg-transparent p-3 font-mono text-[13px] leading-5 text-fg-strong outline-none"
      />

      <div className="flex items-center gap-2 border-t border-line px-3 py-2 text-[11px] text-faint">
        {/* 编辑器盖住列表时，Palette 底部的状态栏不在 DOM 里 ——
            而「保存失败」的报错恰恰发生在这个视图下，不在这里渲染
            一份，用户只会看到「点了没反应」。着色规则与 Palette 一致：
            ok 绿、warn/err 琥珀（降级不是故障，不制造恐慌） */}
        {status ? (
          <span
            data-status={status.kind}
            className={
              status.kind === "ok"
                ? "text-emerald-400 light:text-emerald-700"
                : "text-amber-400 light:text-amber-700"
            }
          >
            {status.text}
          </span>
        ) : (
          <span>Esc 取消 · ⌘↵ 保存</span>
        )}
        <span className="ml-auto flex items-center gap-2">
          <button
            type="button"
            onClick={closeEditor}
            className="rounded px-2 py-0.5 text-muted transition-colors hover:bg-hover hover:text-fg"
          >
            取消
          </button>
          <button
            type="button"
            data-editor-save=""
            onClick={save}
            disabled={empty}
            title={empty ? "内容不能为空" : undefined}
            className="rounded-md border border-line bg-field px-2.5 py-0.5 text-fg transition-colors enabled:hover:border-sky-400/40 enabled:hover:bg-sky-400/10 enabled:hover:text-sky-200 light:enabled:hover:text-sky-700 disabled:cursor-not-allowed disabled:opacity-40"
          >
            保存
          </button>
        </span>
      </div>
    </div>
  );
}
