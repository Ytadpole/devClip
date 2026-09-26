import { TYPE_META } from "../lib/format";
import type { ContentType } from "../lib/api";

export function TypeBadge({ type, className = "" }: { type: ContentType; className?: string }) {
  const m = TYPE_META[type];
  return (
    <span
      className={`shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium tracking-wide ${m.badge} ${className}`}
    >
      {m.label}
    </span>
  );
}
