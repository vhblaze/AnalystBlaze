import { Folder, Lock, Shield } from "lucide-react";
import type { DiskTreeNodeSummary } from "@/services/tauri/agent";

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

/** Colors validated with the dataviz skill's palette checker (categorical
 * pair, dark mode, slate-950 surface): folders=cyan-600, files=violet-600
 * both clear the OKLCH lightness band + CVD separation checks. Protected/
 * system items use the app's existing amber "watch" status tone instead of
 * a third categorical hue - status is reserved, never part of the identity
 * pair (see dataviz skill: status colors ship with icon+label, not color
 * alone). Text never carries the fill color, only slate/amber ink tokens. */
const DIR_FILL = "bg-cyan-600/80 hover:bg-cyan-500/90 border-cyan-400/30";
const FILE_FILL = "bg-violet-600/70 hover:bg-violet-500/80 border-violet-400/30";
const LOCKED_FILL = "bg-amber-600/40 hover:bg-amber-500/50 border-amber-400/30";

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value >= 100 || unitIndex === 0 ? Math.round(value) : value.toFixed(1)} ${units[unitIndex]}`;
}

export function DiskTreemap({
  items,
  onOpen,
  minLabelArea = 2200,
}: {
  items: DiskTreeNodeSummary[];
  onOpen: (item: DiskTreeNodeSummary) => void;
  /** Below this pixel-area, a rect only shows on hover (via title) instead
   * of an inline label, so tiny slivers don't overflow with text. */
  minLabelArea?: number;
}) {
  const sorted = [...items].sort((a, b) => b.sizeBytes - a.sizeBytes);
  const byId = new Map(sorted.map((item) => [item.path, item]));
  const rects = squarify(
    sorted.map((item) => ({ id: item.path, value: item.sizeBytes })),
    1000,
    620,
  );

  if (rects.length === 0) {
    return null;
  }

  return (
    <div className="relative w-full overflow-hidden rounded-xl border border-cyan-500/10 bg-slate-950/40" style={{ aspectRatio: "1000 / 620" }}>
      {rects.map((rect) => {
        const item = byId.get(rect.id);
        if (!item) return null;
        const locked = !item.actionable;
        const fill = locked ? LOCKED_FILL : item.isDir ? DIR_FILL : FILE_FILL;
        const area = rect.w * rect.h;
        const showLabel = area >= minLabelArea;
        const clickable = item.isDir;
        return (
          <button
            key={rect.id}
            type="button"
            title={`${item.name} - ${formatBytes(item.sizeBytes)}${locked ? " (protegido/sistema, somente informativo)" : ""}`}
            disabled={!clickable}
            onClick={() => clickable && onOpen(item)}
            className={`group absolute overflow-hidden border text-left transition-colors ${fill} ${clickable ? "cursor-pointer" : "cursor-default"}`}
            style={{
              left: `${(rect.x / 1000) * 100}%`,
              top: `${(rect.y / 620) * 100}%`,
              width: `${(rect.w / 1000) * 100}%`,
              height: `${(rect.h / 620) * 100}%`,
              margin: "1px",
            }}
          >
            {showLabel && (
              <div className="pointer-events-none flex h-full flex-col justify-between p-1.5">
                <div className="flex items-center gap-1 truncate">
                  {locked ? (
                    item.protected ? (
                      <Shield className="h-3 w-3 shrink-0 text-amber-200" />
                    ) : (
                      <Lock className="h-3 w-3 shrink-0 text-amber-200" />
                    )
                  ) : item.isDir ? (
                    <Folder className="h-3 w-3 shrink-0 text-cyan-100" />
                  ) : null}
                  <span className="truncate text-[11px] font-medium text-slate-50">{item.name}</span>
                </div>
                <span className="truncate font-mono text-[10px] text-slate-200/80">{formatBytes(item.sizeBytes)}</span>
              </div>
            )}
          </button>
        );
      })}
    </div>
  );
}

export function DiskTreemapLegend({ t }: { t: (key: string) => string }) {
  return (
    <div className="flex flex-wrap items-center gap-4 font-mono text-[10px] uppercase tracking-widest text-slate-500">
      <span className="flex items-center gap-1.5">
        <span className="h-2.5 w-2.5 rounded-sm bg-cyan-600" />
        {t("diskExplorer.legendFolders")}
      </span>
      <span className="flex items-center gap-1.5">
        <span className="h-2.5 w-2.5 rounded-sm bg-violet-600" />
        {t("diskExplorer.legendFiles")}
      </span>
      <span className="flex items-center gap-1.5">
        <span className="h-2.5 w-2.5 rounded-sm bg-amber-600" />
        {t("diskExplorer.legendLocked")}
      </span>
    </div>
  );
}
