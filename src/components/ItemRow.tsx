import { Command } from "cmdk";
import { TypeBadge } from "./TypeBadge";
import { TYPE_META, bytes, oneLine, relTime } from "../lib/format";
import type { ClipboardItem } from "../lib/api";

interface Props {
  item: ClipboardItem;
  active: boolean;
  onSelect: (id: number) => void;
  onToggleFavorite: (id: number) => void;
}

export function ItemRow({ item, active, onSelect, onToggleFavorite }: Props) {
  return (
    <Command.Item
      value={String(item.id)}
      onSelect={() => onSelect(item.id)}
      className={`group relative flex cursor-pointer items-start gap-3 border-l-2 px-3 py-2.5 transition-colors ${
        active
          ? "border-sky-400 bg-white/[0.07]"
          : "border-transparent hover:bg-white/[0.035]"
      }`}
    >
      <TypeBadge type={item.contentType} className="mt-0.5" />

      <div className="min-w-0 flex-1">
        <div
          className={`truncate font-mono text-[13px] leading-5 ${
            active ? "text-zinc-100" : "text-zinc-300"
          }`}
        >
          {oneLine(item.content, 200)}
        </div>

        <div className="mt-1 flex items-center gap-2 text-[11px] text-zinc-500">
          <span>{relTime(item.lastCopiedAt)}</span>
          {item.sourceApp && (
            <>
              <span className="text-zinc-700">·</span>
              <span className="truncate">{item.sourceApp}</span>
            </>
          )}
          {item.copyCount > 1 && (
            <>
              <span className="text-zinc-700">·</span>
              <span>×{item.copyCount}</span>
            </>
          )}
          <span className="text-zinc-700">·</span>
          <span>{bytes(item.byteSize)}</span>
          {item.sensitive && (
            <>
              <span className="text-zinc-700">·</span>
              <span className="text-amber-500/90" title="疑似敏感信息（密钥 / token）">
                🔒 敏感
              </span>
            </>
          )}
        </div>
      </div>

      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          onToggleFavorite(item.id);
        }}
        className="mt-0.5 shrink-0 rounded p-1 text-zinc-600 transition-colors hover:bg-white/10 hover:text-amber-300"
        aria-label={item.favorite ? "取消收藏" : "收藏"}
      >
        <svg viewBox="0 0 24 24" className="h-3.5 w-3.5" fill={item.favorite ? "currentColor" : "none"} stroke="currentColor" strokeWidth="2">
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            d="M12 3.5l2.6 5.4 5.9.8-4.3 4.1 1 5.9-5.2-2.8-5.2 2.8 1-5.9L3.5 9.7l5.9-.8z"
          />
        </svg>
      </button>

      {/* 选中态左侧色条，呼应类型色 */}
      <span
        className={`absolute left-0 h-full w-[2px] ${TYPE_META[item.contentType].bar} ${
          active ? "opacity-100" : "opacity-0"
        }`}
        aria-hidden
      />
    </Command.Item>
  );
}
