import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { Command } from "cmdk";
import { ItemRow } from "./ItemRow";
import { useStore } from "../store";

/**
 * 行高。**必须**与 ItemRow 上的 `h-[62px]` 一致，
 * 两边对不上的话窗口就会算错，表现为滚动时行错位。
 *
 * 62 = 左边框 2 + py-2.5 上下 20 + 内容 40（leading-5 的 20 +
 * mt-1 的 4 + leading-4 的 16）。Tailwind 是 border-box，
 * 边框和 padding 都算在这 62px 里面。
 */
const ROW_H = 62;

/** 视口高度初值，layout effect 里会用真实 clientHeight 覆盖 */
const VIEW_H_SEED = 420;

/** 上下各多渲染几行，滚轮没到边界时不会看到白 */
const OVERSCAN = 4;

/** Command.List 上的 p-1.5。scrollTop 从 padding 内侧算起，
 *  而行的位置要从 padding 外侧算，两者差这 6px；
 *  不补上的话窗口会算偏，边界上会露出半行。 */
const PAD_Y = 6;

type Range = [start: number, end: number];

/**
 * 要渲染的区间。正常只有一段（跟随滚动位置），
 * 选中项被滚出窗口时额外插一小段把它钉住。
 */
function windows(scrollTop: number, viewH: number, total: number, pinned: number): Range[] {
  // scrollTop 含 PAD_Y 的偏移，换算成内容坐标
  const at = scrollTop - PAD_Y;
  const main: Range = [
    Math.max(0, Math.floor(at / ROW_H) - OVERSCAN),
    Math.min(total, Math.ceil((at + viewH) / ROW_H) + OVERSCAN),
  ];
  if (total === 0 || (pinned >= main[0] && pinned < main[1])) return [main];

  // 钉住区间要和 main 不重叠，否则会渲染出重复的行
  const pin: Range =
    pinned < main[0]
      ? [Math.max(0, pinned - 1), Math.min(total, main[0], pinned + 2)]
      : [Math.max(main[1], pinned - 1), Math.min(total, pinned + 2)];
  return pin[1] > pin[0] ? (pinned < main[0] ? [pin, main] : [main, pin]) : [main];
}

/**
 * 虚拟列表
 *
 * 为什么手写而不用 react-virtual：行高固定、容器高度固定，
 * 窗口计算就十几行，为此引一个依赖不划算。
 *
 * 和 cmdk 的分工：cmdk 定位当前项靠的是在 DOM 里查
 *  `[cmdk-item][aria-selected="true"]`（见 cmdk 的 k()/G()），
 *  ↑↓ 也是在「已渲染的项」之间前后跳。虚拟化之后它只看得到
 *  窗口内的项，这两套逻辑都会算错，所以键盘导航由 Palette 接管，
 *  cmdk 只保留 Input / Empty / Item 的 aria 与点击行为。
 *
 * 因此有一条硬约束：**selected 那一项必须始终留在 DOM 里**，
 * 否则 cmdk 找不到它，Enter 会静默失效。滚动时用 pin 兜住，
 * 键盘移动时用下面的 effect 拉进可视区。
 */
export function VirtualList() {
  const items = useStore((s) => s.items);
  const selected = useStore((s) => s.selected);
  const loading = useStore((s) => s.loading);
  const query = useStore((s) => s.query);
  const select = useStore((s) => s.select);
  const paste = useStore((s) => s.paste);
  const toggleFavorite = useStore((s) => s.toggleFavorite);
  const openMenu = useStore((s) => s.openMenu);

  // Command.List 的外层 div 就是滚动容器，ref 转发给了我们
  const viewport = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewH, setViewH] = useState(VIEW_H_SEED);

  useLayoutEffect(() => {
    const el = viewport.current;
    if (!el) return;
    const measure = () => setViewH(Math.max(1, el.clientHeight));
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, []);

  // 键盘移动选中项时把它带进可视区。只在 selected 变化时跑，
  // 所以用户自己滚走不会被拽回来
  useLayoutEffect(() => {
    const el = viewport.current;
    if (!el || items.length === 0) return;
    const top = PAD_Y + selected * ROW_H;
    if (top < el.scrollTop) el.scrollTop = top;
    else if (top + ROW_H > el.scrollTop + el.clientHeight) {
      el.scrollTop = top + ROW_H - el.clientHeight;
    }
  }, [selected, items]);

  const rows: ReactNode[] = [];
  let cursor = 0;
  windows(scrollTop, viewH, items.length, selected).forEach(([from, to], wi) => {
    const gap = (from - cursor) * ROW_H;
    cursor = to;
    if (gap > 0) {
      rows.push(<div key={`gap-${wi}`} style={{ height: gap }} aria-hidden />);
    }
    for (let i = from; i < to; i++) {
      const item = items[i];
      rows.push(
        <ItemRow
          key={item.id}
          item={item}
          active={i === selected}
          onSelect={(id) => void paste(id)}
          onToggleFavorite={(id) => void toggleFavorite(id)}
          onContextMenu={(it, x, y) => {
            select(i);
            openMenu(it, x, y);
          }}
        />,
      );
    }
  });
  const tail = (items.length - cursor) * ROW_H;
  if (tail > 0) {
    rows.push(<div key="gap-tail" style={{ height: tail }} aria-hidden />);
  }

  return (
    <Command.List
      ref={viewport}
      onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}
      className="min-h-0 flex-1 overflow-y-auto overscroll-contain p-1.5"
    >
      {loading ? (
        <div className="px-3 py-10 text-center text-sm text-faint">加载中…</div>
      ) : (
        <>
          <Command.Empty className="px-3 py-10 text-center text-sm text-faint">
            没有匹配「{query}」的记录
          </Command.Empty>
          {rows}
        </>
      )}
    </Command.List>
  );
}
