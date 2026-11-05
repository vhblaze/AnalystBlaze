import { Lock, Shield } from "lucide-react";
import type { DiskTreeNodeSummary } from "@/services/tauri/agent";
import { formatBytes } from "@/lib/format";
import { cn } from "@/lib/utils";

/* Layout copiado de analystblaze/DiskTreemap.tsx (a view antiga sai depois). */
/** Squarified treemap layout (Bruls, Huizing & van Wijk, 1999) - lays out
 * rectangles proportional to `value` while keeping aspect ratios close to
 * square, which is what makes a treemap scannable instead of a strip of
 * slivers. Items must already be sorted descending by value for a good
 * layout (not required for correctness, just for visual quality). */
interface TreemapInput {
  id: string;
  value: number;
}

interface TreemapRect {
  id: string;
  value: number;
  x: number;
  y: number;
  w: number;
  h: number;
}

function worstRatio(row: { area: number }[], shortSide: number): number {
  const sum = row.reduce((total, item) => total + item.area, 0);
  const max = Math.max(...row.map((item) => item.area));
  const min = Math.min(...row.map((item) => item.area));
  if (sum <= 0 || min <= 0 || shortSide <= 0) return Infinity;
  return Math.max((shortSide * shortSide * max) / (sum * sum), (sum * sum) / (shortSide * shortSide * min));
}

function layout(
  items: { id: string; value: number; area: number }[],
  x: number,
  y: number,
  w: number,
  h: number,
  out: TreemapRect[],
) {
  if (items.length === 0 || w <= 0 || h <= 0) return;
  if (items.length === 1) {
    out.push({ id: items[0].id, value: items[0].value, x, y, w, h });
    return;
  }

  const shortSide = Math.min(w, h);
  let row = items.slice(0, 1);
  let rowWorst = worstRatio(row, shortSide);
  let i = 1;
  while (i < items.length) {
    const candidate = items.slice(0, i + 1);
    const candidateWorst = worstRatio(candidate, shortSide);
    if (candidateWorst > rowWorst) break;
    row = candidate;
    rowWorst = candidateWorst;
    i += 1;
  }

  const rowArea = row.reduce((sum, item) => sum + item.area, 0);
  const rest = items.slice(row.length);

  if (w >= h) {
    const rowWidth = h > 0 ? rowArea / h : 0;
    let cursorY = y;
    for (const item of row) {
      const itemHeight = rowWidth > 0 ? item.area / rowWidth : 0;
      out.push({ id: item.id, value: item.value, x, y: cursorY, w: rowWidth, h: itemHeight });
      cursorY += itemHeight;
    }
    layout(rest, x + rowWidth, y, w - rowWidth, h, out);
  } else {
    const rowHeight = w > 0 ? rowArea / w : 0;
    let cursorX = x;
    for (const item of row) {
      const itemWidth = rowHeight > 0 ? item.area / rowHeight : 0;
      out.push({ id: item.id, value: item.value, x: cursorX, y, w: itemWidth, h: rowHeight });
      cursorX += itemWidth;
    }
    layout(rest, x, y + rowHeight, w, h - rowHeight, out);
  }
}

export function squarify(items: TreemapInput[], w: number, h: number): TreemapRect[] {
  const filtered = items.filter((item) => item.value > 0);
  if (filtered.length === 0 || w <= 0 || h <= 0) return [];
  const total = filtered.reduce((sum, item) => sum + item.value, 0);
  if (total <= 0) return [];
  const scale = (w * h) / total;
  const scaled = filtered.map((item) => ({ ...item, area: item.value * scale }));
  const out: TreemapRect[] = [];
  layout(scaled, 0, 0, w, h, out);
  return out;
}

/** Espaço lógico do layout; o mapa escala por porcentagem. */
const LAYOUT_W = 1000;
const LAYOUT_H = 840;

/**
 * Cores por token (doc 15 §3): pastas = accent, arquivos = violet,
 * protegido/do Windows = warn. O texto fica sempre em fg, nunca na cor do
 * preenchimento; o status protegido vem também com ícone (Shield/Lock).
 */
const FILL = {
  dir: "border-accent/45 bg-accent/15 hover:bg-accent/25",
  file: "border-violet/45 bg-violet/15",
  locked: "border-warn/50 bg-warn/15",
} as const;

export function DiskTreemap({
  items,
  onOpen,
  label,
  lockedHint,
  minLabelArea = 9000,
  className,
}: {
  items: DiskTreeNodeSummary[];
  onOpen: (item: DiskTreeNodeSummary) => void;
  /** Rótulo para leitor de tela ("Mapa do espaço desta pasta"). */
  label: string;
  /** Sufixo do title dos itens que não dá para apagar. */
  lockedHint: string;
  /** Abaixo desta área (no espaço 1000×840) o retângulo só mostra o nome no title. */
  minLabelArea?: number;
  className?: string;
}) {
  const sorted = [...items].sort((a, b) => b.sizeBytes - a.sizeBytes);
  const byId = new Map(sorted.map((item) => [item.path, item]));
  const rects = squarify(
    sorted.map((item) => ({ id: item.path, value: item.sizeBytes })),
    LAYOUT_W,
    LAYOUT_H,
  );

  if (rects.length === 0) return null;

  return (
    <div
      role="group"
      aria-label={label}
      className={cn("relative w-full overflow-hidden rounded-control", className)}
      style={{ aspectRatio: `${LAYOUT_W} / ${LAYOUT_H}` }}
    >
      {rects.map((rect) => {
        const item = byId.get(rect.id);
        if (!item) return null;
        const locked = !item.actionable;
        const fill = locked ? FILL.locked : item.isDir ? FILL.dir : FILL.file;
        const showLabel = rect.w * rect.h >= minLabelArea && rect.w >= 60;
        const clickable = item.isDir;
        return (
          <button
            key={rect.id}
            type="button"
            title={`${item.name} · ${formatBytes(item.sizeBytes)}${locked ? ` · ${lockedHint}` : ""}`}
            aria-label={`${item.name} · ${formatBytes(item.sizeBytes)}`}
            disabled={!clickable}
            onClick={() => clickable && onOpen(item)}
            className={cn(
              "absolute overflow-hidden rounded-[6px] border text-left transition-colors duration-fast ease-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60",
              fill,
              clickable ? "cursor-pointer" : "cursor-default",
            )}
            style={{
              left: `calc(${(rect.x / LAYOUT_W) * 100}% + 2px)`,
              top: `calc(${(rect.y / LAYOUT_H) * 100}% + 2px)`,
              width: `calc(${(rect.w / LAYOUT_W) * 100}% - 4px)`,
              height: `calc(${(rect.h / LAYOUT_H) * 100}% - 4px)`,
            }}
          >
            {showLabel && (
              <div className="pointer-events-none flex h-full flex-col justify-between p-2">
                <div className="flex min-w-0 items-center gap-1">
                  {locked &&
                    (item.protected ? (
                      <Shield className="size-3 shrink-0 text-warn" aria-hidden />
                    ) : (
                      <Lock className="size-3 shrink-0 text-warn" aria-hidden />
                    ))}
                  <span className="truncate text-[12px] font-medium text-fg">{item.name}</span>
                </div>
                <span className="truncate text-[11px] tabular-nums text-fg-2">{formatBytes(item.sizeBytes)}</span>
              </div>
            )}
          </button>
        );
      })}
    </div>
  );
}

export function DiskTreemapLegend({
  folders,
  files,
  locked,
  className,
}: {
  folders: string;
  files: string;
  locked: string;
  className?: string;
}) {
  return (
    <div className={cn("flex flex-wrap items-center gap-4 text-xs text-fg-3", className)}>
      <span className="flex items-center gap-1.5">
        <span className="size-2.5 rounded-[3px] bg-accent" aria-hidden />
        {folders}
      </span>
      <span className="flex items-center gap-1.5">
        <span className="size-2.5 rounded-[3px] bg-violet" aria-hidden />
        {files}
      </span>
      <span className="flex items-center gap-1.5">
        <span className="size-2.5 rounded-[3px] bg-warn" aria-hidden />
        {locked}
      </span>
    </div>
  );
}
