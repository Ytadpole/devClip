import { useLayoutEffect, useRef, useState } from "react";
import { useStore } from "../store";

type Entry =
  | { kind: "sep" }
  | {
      kind: "item";
      label: string;
      hint: string;
      danger?: boolean;
      run: () => void;
    };

/**
 * 右键「更多」菜单
 *
 * 挂在 body 上用 fixed 定位：放在列表里会被 overflow 裁掉。
 * Esc 由 Palette 的全局 handler 收口，这里只管点外面和窗口变化。
 */
export function ContextMenu() {
  const menu = useStore((s) => s.menu);
  const close = useStore((s) => s.closeMenu);
  const copy = useStore((s) => s.copy);
  const paste = useStore((s) => s.paste);
  const toggleFavorite = useStore((s) => s.toggleFavorite);
  const remove = useStore((s) => s.remove);
  const beginEdit = useStore((s) => s.beginEdit);

  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x: 0, y: 0 });

  // 贴边时向内收，避免菜单被窗口裁掉
  useLayoutEffect(() => {
    if (!menu) return;
    const el = ref.current;
    if (!el) return;
    setPos({
      x: Math.max(8, Math.min(menu.x, window.innerWidth - el.offsetWidth - 8)),
      y: Math.max(8, Math.min(menu.y, window.innerHeight - el.offsetHeight - 8)),
    });
  }, [menu]);

  useLayoutEffect(() => {
    if (!menu) return;
    const onDown = (e: PointerEvent) => {
      if (!ref.current?.contains(e.target as Node)) close();
    };
    window.addEventListener("pointerdown", onDown);
    window.addEventListener("resize", close);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("pointerdown", onDown);
      window.removeEventListener("resize", close);
      window.removeEventListener("blur", close);
    };
  }, [menu, close]);

  if (!menu) return null;

  const it = menu.item;
  const entries: Entry[] = [
    { kind: "item", label: "粘贴", hint: "↵", run: () => void paste(it.id) },
    { kind: "item", label: "复制", hint: "⌘C", run: () => void copy(it.id) },
    // 图片条目没有可编辑的文本（content 是占位），入口就不给
    ...(it.contentType === "image"
      ? []
      : [{ kind: "item", label: "编辑", hint: "⌘E", run: () => beginEdit(it) } satisfies Entry]),
    {
      kind: "item",
      label: it.favorite ? "取消收藏" : "收藏",
      hint: "⌘D",
      run: () => void toggleFavorite(it.id),
    },
    { kind: "sep" },
    { kind: "item", label: "删除", hint: "⌘⌫", danger: true, run: () => void remove(it.id) },
  ];

  return (
    <div
      ref={ref}
      role="menu"
      style={{ left: pos.x, top: pos.y }}
      // 右键打开时，浏览器自带的菜单要挡掉
      onContextMenu={(e) => e.preventDefault()}
      className="fixed z-50 min-w-[176px] rounded-lg border border-line bg-panel/95 py-1 shadow-xl shadow-black/60 backdrop-blur-xl light:shadow-black/10"
    >
      {entries.map((e, i) =>
        e.kind === "sep" ? (
          <div key={i} className="my-1 h-px bg-line" />
        ) : (
          <button
            key={i}
            type="button"
            role="menuitem"
            onClick={() => {
              close();
              e.run();
            }}
            className={`flex w-full items-center gap-6 px-3 py-1.5 text-left text-[13px] transition-colors ${
              e.danger
                ? "text-rose-300 hover:bg-rose-500/15 light:text-rose-700"
                : "text-fg-strong hover:bg-active"
            }`}
          >
            <span className="flex-1">{e.label}</span>
            <span className="text-[11px] text-faint">{e.hint}</span>
          </button>
        ),
      )}
    </div>
  );
}
