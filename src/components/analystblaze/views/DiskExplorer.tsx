import { useEffect, useState } from "react";
import { AlertTriangle, ChevronRight, File, Folder, HardDrive, Lock, RefreshCw, Shield, Trash2, X } from "lucide-react";
import { useI18n } from "@/i18n";
import { useTelemetry } from "@/hooks/useTelemetry";
import {
  cancelDiskTreeScan,
  deleteDiskUsageItem,
  getDiskTreeChildren,
  getDiskTreeNode,
  isTauriRuntime,
  listDiskVolumes,
  listenToDiskTreeProgress,
  scanDiskTree,
  type DiskTreeNodeSummary,
  type DiskTreeProgress,
  type DiskTreeScanSummary,
  type DiskVolumeInfo,
} from "@/services/tauri/agent";
import { DiskTreemap, DiskTreemapLegend } from "@/components/analystblaze/DiskTreemap";

function errorMessage(error: unknown) {
  if (error instanceof Error) return error.message;
  return String(error);
}

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

function formatTimeAgo(unixSeconds: number): string {
  const deltaSeconds = Math.max(0, Math.floor(Date.now() / 1000) - unixSeconds);
  if (deltaSeconds < 60) return "poucos segundos";
  const minutes = Math.floor(deltaSeconds / 60);
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} h`;
  const days = Math.floor(hours / 24);
  return `${days} d`;
}

function parentPath(path: string): string | null {
  const trimmed = path.replace(/[\\/]+$/, "");
  const lastSep = Math.max(trimmed.lastIndexOf("\\"), trimmed.lastIndexOf("/"));
  if (lastSep <= 2) return null; // hit a bare drive root like "C:\"
  return trimmed.slice(0, lastSep);
}

function breadcrumbSegments(root: string, current: string): { label: string; path: string }[] {
  if (!current.startsWith(root)) return [{ label: root, path: root }];
  const rest = current.slice(root.length).replace(/^[\\/]+/, "");
  const segments = [{ label: root, path: root }];
  if (!rest) return segments;
  let cursor = root.replace(/[\\/]+$/, "");
  for (const part of rest.split(/[\\/]+/).filter(Boolean)) {
    cursor = `${cursor}\\${part}`;
    segments.push({ label: part, path: cursor });
  }
  return segments;
}

export function DiskExplorer({
  autoScan,
  onAutoScanHandled,
}: {
  /** Set (transiently) when the user navigated here from an Insights card
   * asking to see disk-usage details - triggers a scan of the first
   * detected volume on arrival. */
  autoScan?: boolean;
  onAutoScanHandled?: () => void;
}) {
  const { t } = useI18n();
  const track = useTelemetry("disk_explorer");
  const runtimeAvailable = isTauriRuntime();

  const [volumes, setVolumes] = useState<DiskVolumeInfo[]>([]);
  const [selectedVolume, setSelectedVolume] = useState<string>("");
  const [volumesError, setVolumesError] = useState<string | null>(null);

  const [scanSummary, setScanSummary] = useState<DiskTreeScanSummary | null>(null);
  const [scanBusy, setScanBusy] = useState(false);
  const [scanError, setScanError] = useState<string | null>(null);
  const [scanProgress, setScanProgress] = useState<DiskTreeProgress | null>(null);

  const [currentPath, setCurrentPath] = useState<string>("");
  const [currentNode, setCurrentNode] = useState<DiskTreeNodeSummary | null>(null);
  const [children, setChildren] = useState<DiskTreeNodeSummary[]>([]);
  const [browseBusy, setBrowseBusy] = useState(false);
  const [browseError, setBrowseError] = useState<string | null>(null);

  const [sortBy, setSortBy] = useState<"size" | "name">("size");
  const [confirmingDeletePath, setConfirmingDeletePath] = useState<string | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);

  useEffect(() => {
    if (!runtimeAvailable) return;
    listDiskVolumes()
      .then((list) => {
        setVolumes(list);
        setSelectedVolume((current) => current || list[0]?.mountPoint || "");
      })
      .catch((error) => setVolumesError(errorMessage(error)));
  }, [runtimeAvailable]);

  useEffect(() => {
    if (!runtimeAvailable) return;
    let dispose: (() => void) | undefined;
    listenToDiskTreeProgress((progress) => setScanProgress(progress)).then((next) => {
      dispose = next;
    });
    return () => dispose?.();
  }, [runtimeAvailable]);

  useEffect(() => {
    if (!autoScan || volumes.length === 0 || scanSummary || scanBusy) return;
    void startScan(selectedVolume || volumes[0].mountPoint);
    onAutoScanHandled?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [autoScan, volumes]);

  const openPath = async (path: string) => {
    setBrowseBusy(true);
    setBrowseError(null);
    try {
      const [node, kids] = await Promise.all([getDiskTreeNode(path), getDiskTreeChildren(path)]);
      setCurrentPath(path);
      setCurrentNode(node);
      setChildren(kids);
    } catch (error) {
      setBrowseError(errorMessage(error));
    } finally {
      setBrowseBusy(false);
    }
  };

  const startScan = async (volumeOverride?: string) => {
    const volume = volumeOverride ?? selectedVolume;
    if (!volume) return;
    setScanBusy(true);
    setScanError(null);
    setScanProgress(null);
    setActionMessage(null);
    try {
      const summary = await scanDiskTree(volume);
      setScanSummary(summary);
      track("disk_tree_scanned", { canceled: summary.canceled, capped: summary.capped, fileCount: summary.fileCount });
      await openPath(summary.root);
    } catch (error) {
      setScanError(errorMessage(error));
    } finally {
      setScanBusy(false);
    }
  };

  const cancelScan = async () => {
    try {
      await cancelDiskTreeScan();
    } catch (error) {
      setScanError(errorMessage(error));
    }
  };

  const goUp = () => {
    if (!scanSummary) return;
    const parent = parentPath(currentPath);
    if (!parent || parent.length < scanSummary.root.length) return;
    void openPath(parent);
  };

  const deleteItem = async (item: DiskTreeNodeSummary) => {
    setConfirmingDeletePath(null);
    setActionMessage(null);
    try {
      const result = await deleteDiskUsageItem(item.path);
      const outcome = result as { success?: boolean; message?: string } | undefined;
      if (outcome && outcome.success === false && outcome.message) {
        setActionMessage(outcome.message);
        return;
      }
      // The scanned tree cache isn't rewritten by a delete - drop the item
      // from the current view immediately; ancestor totals stay as they
      // were at scan time until the next "Analisar novamente".
      setChildren((current) => current.filter((child) => child.path !== item.path));
      setActionMessage(t("diskExplorer.deleteSuccess", { name: item.name }));
      track("disk_tree_item_deleted", { isDir: item.isDir });
    } catch (error) {
      setActionMessage(errorMessage(error));
    }
  };

  const sortedChildren = [...children].sort((a, b) =>
    sortBy === "size" ? b.sizeBytes - a.sizeBytes : a.name.localeCompare(b.name),
  );

  if (!runtimeAvailable) {
    return <Notice tone="info" message={t("diskExplorer.desktopOnly")} />;
  }

  return (
    <div className="flex flex-col gap-8">
      <header className="flex flex-col gap-2">
        <div className="flex items-center gap-2 font-mono text-[11px] uppercase tracking-[0.3em] text-cyan-400/70">
          <HardDrive className="h-3 w-3" />
          {t("diskExplorer.eyebrow")}
        </div>
        <h1 className="text-[36px] font-semibold tracking-tight text-slate-50">{t("diskExplorer.title")}</h1>
        <p className="max-w-2xl text-sm text-slate-400">{t("diskExplorer.description")}</p>
      </header>

      <section className="glass-panel cyber-glow p-6">
        <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
          <div className="flex flex-wrap items-center gap-2">
            {volumesError && <Notice tone="danger" message={volumesError} />}
            {volumes.map((volume) => {
              const usedRatio = volume.totalBytes > 0 ? 1 - volume.availableBytes / volume.totalBytes : 0;
              const selected = volume.mountPoint === selectedVolume;
              return (
                <button
                  key={volume.mountPoint}
                  onClick={() => setSelectedVolume(volume.mountPoint)}
                  disabled={scanBusy}
                  className={`flex min-w-[180px] flex-col gap-1.5 rounded-xl border px-3.5 py-2.5 text-left text-xs transition-all disabled:opacity-50 ${
                    selected
                      ? "border-cyan-300/50 bg-cyan-400/10 text-cyan-100"
                      : "border-cyan-500/10 bg-slate-950/40 text-slate-300 hover:border-cyan-400/30"
                  }`}
                >
                  <span className="flex items-center justify-between font-semibold">
                    <span className="truncate">{volume.label}</span>
                    <span className="font-mono text-[10px] text-slate-500">{volume.fileSystem || "--"}</span>
                  </span>
                  <span className="h-1.5 overflow-hidden rounded-full bg-slate-800">
                    <span
                      className="block h-full rounded-full bg-gradient-to-r from-cyan-400 to-violet-400"
                      style={{ width: `${Math.max(2, Math.min(100, usedRatio * 100))}%` }}
                    />
                  </span>
                  <span className="font-mono text-[10px] text-slate-500">
                    {formatBytes(volume.totalBytes - volume.availableBytes)} / {formatBytes(volume.totalBytes)}
                  </span>
                </button>
              );
            })}
          </div>

          <div className="flex items-center gap-2">
            {scanBusy && (
              <button
                onClick={() => void cancelScan()}
                className="inline-flex items-center gap-2 rounded-xl border border-rose-400/30 bg-rose-400/10 px-3 py-2.5 text-xs font-semibold text-rose-100 transition-all hover:border-rose-300/50"
              >
                <X className="h-3.5 w-3.5" />
                {t("common.cancel")}
              </button>
            )}
            <button
              disabled={scanBusy || !selectedVolume}
              onClick={() => void startScan()}
              className="group inline-flex items-center gap-2 rounded-xl border border-cyan-400/40 bg-gradient-to-r from-cyan-500/20 to-violet-500/10 px-4 py-2.5 text-sm font-semibold text-cyan-100 transition-all hover:border-cyan-300/60 disabled:opacity-50"
            >
              <RefreshCw className={`h-4 w-4 ${scanBusy ? "animate-spin" : "transition-transform group-hover:rotate-180"}`} />
              {scanBusy ? t("diskExplorer.scanning") : scanSummary ? t("diskExplorer.rescan") : t("diskExplorer.scan")}
            </button>
          </div>
        </div>

        {scanBusy && (
          <div className="mt-4 flex items-center gap-3 text-xs text-slate-400">
            <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-slate-800">
              <div className="h-full w-1/3 animate-pulse rounded-full bg-gradient-to-r from-cyan-400 to-violet-400" />
            </div>
            <span className="font-mono">
              {t("diskExplorer.scannedNodes", { count: scanProgress?.scannedNodes ?? 0 })}
            </span>
          </div>
        )}
        {scanError && <div className="mt-4"><Notice tone="danger" message={scanError} /></div>}

        {scanSummary && (
          <p className="mt-4 text-xs text-slate-500">
            {t("diskExplorer.lastScanned", { time: formatTimeAgo(scanSummary.scannedAt) })}
            {scanSummary.canceled ? ` - ${t("diskExplorer.scanCanceled")}` : ""}
            {scanSummary.capped ? ` - ${t("diskExplorer.scanCapped")}` : ""}
          </p>
        )}
      </section>

      {!scanSummary && !scanBusy && (
        <div className="glass-panel flex flex-col items-center gap-2 rounded-2xl border border-cyan-500/10 p-10 text-center">
          <HardDrive className="h-8 w-8 text-cyan-300/60" />
          <h3 className="text-lg font-semibold text-slate-100">{t("diskExplorer.emptyTitle")}</h3>
          <p className="max-w-sm text-sm text-slate-400">{t("diskExplorer.emptyDescription")}</p>
        </div>
      )}

      {scanSummary && (
        <section className="glass-panel cyber-glow p-6">
          <div className="flex flex-wrap items-center gap-1 pb-4 font-mono text-xs text-slate-400">
            {breadcrumbSegments(scanSummary.root, currentPath).map((segment, index, all) => (
              <span key={segment.path} className="flex items-center gap-1">
                <button
                  onClick={() => void openPath(segment.path)}
                  disabled={segment.path === currentPath}
                  className={`rounded px-1.5 py-0.5 transition ${
                    segment.path === currentPath ? "text-cyan-200" : "text-slate-400 hover:text-cyan-200"
                  }`}
                >
                  {segment.label}
                </button>
                {index < all.length - 1 && <ChevronRight className="h-3 w-3 text-slate-600" />}
              </span>
            ))}
          </div>

          {browseError && <Notice tone="danger" message={browseError} />}
          {actionMessage && <div className="mb-4"><Notice tone="info" message={actionMessage} /></div>}

          {currentNode && (
            <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
              <div className="flex items-center gap-3">
                <button
                  onClick={goUp}
                  disabled={currentPath === scanSummary.root}
                  className="inline-flex items-center gap-1.5 rounded-lg border border-cyan-500/20 bg-slate-950/50 px-2.5 py-1.5 text-xs font-medium text-cyan-200 transition hover:border-cyan-400/40 disabled:opacity-40"
                >
                  <ChevronRight className="h-3.5 w-3.5 rotate-180" />
                  {t("diskExplorer.up")}
                </button>
                <span className="text-sm text-slate-300">
                  {t("diskExplorer.folderTotal", { size: formatBytes(currentNode.sizeBytes) })}
                </span>
              </div>
              <DiskTreemapLegend t={t} />
            </div>
          )}

          {browseBusy ? (
            <div className="h-[300px] animate-pulse rounded-xl bg-slate-900/40" />
          ) : sortedChildren.length === 0 ? (
            <p className="py-8 text-center text-sm text-slate-500">{t("diskExplorer.folderEmpty")}</p>
          ) : (
            <>
              <DiskTreemap items={sortedChildren} onOpen={(item) => void openPath(item.path)} />

              <div className="mt-5 flex items-center justify-between">
                <h3 className="font-mono text-[10px] uppercase tracking-widest text-slate-500">
                  {t("diskExplorer.listTitle")}
                </h3>
                <div className="flex items-center gap-1 rounded-lg border border-cyan-500/10 bg-slate-950/40 p-0.5 text-[10px] font-mono uppercase tracking-widest">
                  <button
                    onClick={() => setSortBy("size")}
                    className={`rounded px-2 py-1 ${sortBy === "size" ? "bg-cyan-400/15 text-cyan-100" : "text-slate-500"}`}
                  >
                    {t("diskExplorer.sortSize")}
                  </button>
                  <button
                    onClick={() => setSortBy("name")}
                    className={`rounded px-2 py-1 ${sortBy === "name" ? "bg-cyan-400/15 text-cyan-100" : "text-slate-500"}`}
                  >
                    {t("diskExplorer.sortName")}
                  </button>
                </div>
              </div>

              <div className="mt-2 flex flex-col divide-y divide-cyan-500/5 overflow-hidden rounded-xl border border-cyan-500/10">
                {sortedChildren.map((item) => (
                  <div key={item.path} className="flex items-center gap-3 bg-slate-950/30 px-3 py-2.5 text-sm">
                    {item.isDir ? (
                      <Folder className="h-4 w-4 shrink-0 text-cyan-300" />
                    ) : (
                      <File className="h-4 w-4 shrink-0 text-violet-300" />
                    )}
                    <button
                      disabled={!item.isDir}
                      onClick={() => item.isDir && void openPath(item.path)}
                      className={`min-w-0 flex-1 truncate text-left ${item.isDir ? "text-slate-100 hover:text-cyan-200" : "text-slate-300"}`}
                      title={item.path}
                    >
                      {item.name}
                    </button>
                    {!item.actionable && (
                      <span
                        className="inline-flex items-center gap-1 rounded-md border border-amber-400/30 bg-amber-400/10 px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-widest text-amber-200"
                        title={item.protected ? t("diskExplorer.protectedHint") : t("diskExplorer.systemHint")}
                      >
                        {item.protected ? <Shield className="h-3 w-3" /> : <Lock className="h-3 w-3" />}
                        {t("diskExplorer.locked")}
                      </span>
                    )}
                    <span className="w-20 shrink-0 text-right font-mono text-xs text-slate-400">
                      {formatBytes(item.sizeBytes)}
                    </span>
                    {item.actionable &&
                      (confirmingDeletePath === item.path ? (
                        <div className="flex shrink-0 items-center gap-1">
                          <button
                            onClick={() => void deleteItem(item)}
                            className="rounded-md border border-rose-400/40 bg-rose-400/15 px-2 py-1 font-mono text-[10px] uppercase tracking-widest text-rose-100 hover:bg-rose-400/25"
                          >
                            {t("diskExplorer.confirmDelete")}
                          </button>
                          <button
                            onClick={() => setConfirmingDeletePath(null)}
                            className="rounded-md border border-slate-600/40 px-2 py-1 font-mono text-[10px] uppercase tracking-widest text-slate-400 hover:text-slate-200"
                          >
                            {t("common.cancel")}
                          </button>
                        </div>
                      ) : (
                        <button
                          onClick={() => setConfirmingDeletePath(item.path)}
                          title={t("diskExplorer.deleteHint")}
                          className="shrink-0 rounded-md border border-white/10 bg-slate-950/60 p-1.5 text-slate-500 transition hover:border-rose-400/40 hover:text-rose-200"
                        >
                          <Trash2 className="h-3.5 w-3.5" />
                        </button>
                      ))}
                  </div>
                ))}
              </div>
            </>
          )}
        </section>
      )}
    </div>
  );
}

function Notice({ message, tone }: { message: string; tone: "danger" | "info" | "warning" }) {
  const toneClass =
    tone === "danger"
      ? "border-rose-400/25 bg-rose-400/10 text-rose-100"
      : tone === "warning"
        ? "border-amber-400/25 bg-amber-400/10 text-amber-100"
        : "border-cyan-400/25 bg-cyan-400/10 text-cyan-100";
  return (
    <div className={`flex items-start gap-2 rounded-xl border px-4 py-3 text-sm ${toneClass}`}>
      {tone === "danger" && <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />}
      {message}
    </div>
  );
}
