import { useEffect, useState } from "react";
import { AlertTriangle, ChevronRight, File, Folder, HardDrive, Lock, RefreshCw, Shield, Trash2, X } from "lucide-react";
import { useI18n } from "@/i18n";
import { useTelemetry } from "@/hooks/useTelemetry";
import {
  cancelDiskTreeScan,
  deleteDiskUsageItem,
  isTauriRuntime,
  listDiskDirectory,
  listDiskVolumes,
  listenToDiskTreeProgress,
  type DiskTreeNodeSummary,
  type DiskTreeProgress,
  type DiskVolumeInfo,
} from "@/services/tauri/agent";
import { DiskTreemap, DiskTreemapLegend } from "@/components/analystblaze/DiskTreemap";

/** Mirrors disk_usage.rs's DIRECT_DELETE_THRESHOLD_BYTES - only used here
 * to warn before the click; the backend enforces the real behavior
 * regardless of what this shows. */
const DIRECT_DELETE_THRESHOLD_BYTES = 2 * 1024 * 1024 * 1024;
/** How long the fade/collapse plays before the row actually leaves the
 * list - keep in sync with the CSS transition duration below. */
const DELETE_ANIMATION_MS = 260;

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
   * asking to see disk-usage details - triggers loading the first detected
   * volume's root on arrival. */
  autoScan?: boolean;
  onAutoScanHandled?: () => void;
}) {
  const { t } = useI18n();
  const track = useTelemetry("disk_explorer");
  const runtimeAvailable = isTauriRuntime();

  const [volumes, setVolumes] = useState<DiskVolumeInfo[]>([]);
  const [selectedVolume, setSelectedVolume] = useState<string>("");
  const [volumesError, setVolumesError] = useState<string | null>(null);

  const [rootPath, setRootPath] = useState<string | null>(null);
  const [currentPath, setCurrentPath] = useState<string>("");
  // Nothing here is cached on the backend - every navigation re-asks the
  // filesystem for just this folder's immediate children, so there's no
  // whole-drive tree sitting in memory to leak once you leave the screen.
  const [children, setChildren] = useState<DiskTreeNodeSummary[]>([]);
  const [browseBusy, setBrowseBusy] = useState(false);
  const [browseError, setBrowseError] = useState<string | null>(null);
  const [browseProgress, setBrowseProgress] = useState<DiskTreeProgress | null>(null);

  const [sortBy, setSortBy] = useState<"size" | "name">("size");
  const [confirmingDeletePath, setConfirmingDeletePath] = useState<string | null>(null);
  const [pendingDeletePath, setPendingDeletePath] = useState<string | null>(null);
  const [deletingPaths, setDeletingPaths] = useState<Set<string>>(new Set());
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

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
    listenToDiskTreeProgress((progress) => setBrowseProgress(progress)).then((next) => {
      dispose = next;
    });
    return () => dispose?.();
  }, [runtimeAvailable]);

  useEffect(() => {
    if (!autoScan || volumes.length === 0 || rootPath || browseBusy) return;
    void loadRoot(selectedVolume || volumes[0].mountPoint);
    onAutoScanHandled?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [autoScan, volumes]);

  // Cancel whatever listing is in flight the moment this screen unmounts -
  // otherwise a slow folder (e.g. a huge node_modules) keeps a worker pool
  // busy on a screen the user already left.
  useEffect(() => {
    return () => {
      void cancelDiskTreeScan().catch(() => undefined);
    };
  }, []);

  const openPath = async (path: string) => {
    setBrowseBusy(true);
    setBrowseError(null);
    setBrowseProgress(null);
    try {
      const kids = await listDiskDirectory(path);
      setCurrentPath(path);
      setChildren(kids);
    } catch (error) {
      setBrowseError(errorMessage(error));
    } finally {
      setBrowseBusy(false);
    }
  };

  const loadRoot = async (volumeOverride?: string) => {
    const volume = volumeOverride ?? selectedVolume;
    if (!volume) return;
    setActionMessage(null);
    setRootPath(volume);
    track("disk_tree_root_opened", { volume });
    await openPath(volume);
  };

  const cancelBrowse = async () => {
    try {
      await cancelDiskTreeScan();
    } catch (error) {
      setBrowseError(errorMessage(error));
    }
  };

  const goUp = () => {
    if (!rootPath) return;
    const parent = parentPath(currentPath);
    if (!parent || parent.length < rootPath.length) return;
    void openPath(parent);
  };

  const deleteItem = async (item: DiskTreeNodeSummary) => {
    setConfirmingDeletePath(null);
    setActionMessage(null);
    setActionError(null);
    // A large folder's permanent delete (fs::remove_dir_all over tens of
    // GB / hundreds of thousands of small files, e.g. a cargo target dir)
    // can take real time - show the row as actively deleting right away
    // instead of leaving it looking unresponsive until the promise settles.
    setPendingDeletePath(item.path);
    try {
      const result = await deleteDiskUsageItem(item.path);
      const outcome = result as { success?: boolean; message?: string } | undefined;
      if (outcome && outcome.success === false && outcome.message) {
        // Errors get a modal, not an inline banner - "it failed" is only
        // useful if it also says why, and that deserves the user's full
        // attention rather than scrolling past a small strip of text.
        setActionError(outcome.message);
        return;
      }
      // Backend message already says accurately whether this went to
      // quarantine or was deleted permanently (see DIRECT_DELETE_THRESHOLD)
      // - never paper over that with a hardcoded "quarantine" string.
      setActionMessage(outcome?.message ?? t("diskExplorer.deleteSuccess", { name: item.name }));
      track("disk_tree_item_deleted", { isDir: item.isDir, permanent: item.sizeBytes >= DIRECT_DELETE_THRESHOLD_BYTES });
      // Play the fade/collapse first, then actually drop it from the list -
      // an instant jump-cut read as "it's still there" even though the
      // state was already updated.
      setDeletingPaths((current) => new Set(current).add(item.path));
      window.setTimeout(() => {
        setChildren((current) => current.filter((child) => child.path !== item.path));
        setDeletingPaths((current) => {
          const next = new Set(current);
          next.delete(item.path);
          return next;
        });
      }, DELETE_ANIMATION_MS);
    } catch (error) {
      setActionError(errorMessage(error));
    } finally {
      setPendingDeletePath((current) => (current === item.path ? null : current));
    }
  };

  const sortedChildren = [...children].sort((a, b) =>
    sortBy === "size" ? b.sizeBytes - a.sizeBytes : a.name.localeCompare(b.name),
  );
  const treemapItems = sortedChildren.filter((item) => !deletingPaths.has(item.path));
  const folderTotalBytes = children.reduce((sum, item) => sum + item.sizeBytes, 0);

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
                  disabled={browseBusy}
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
            {browseBusy && (
              <button
                onClick={() => void cancelBrowse()}
                className="inline-flex items-center gap-2 rounded-xl border border-rose-400/30 bg-rose-400/10 px-3 py-2.5 text-xs font-semibold text-rose-100 transition-all hover:border-rose-300/50"
              >
                <X className="h-3.5 w-3.5" />
                {t("common.cancel")}
              </button>
            )}
            <button
              disabled={browseBusy || !selectedVolume}
              onClick={() => void loadRoot()}
              className="group inline-flex items-center gap-2 rounded-xl border border-cyan-400/40 bg-gradient-to-r from-cyan-500/20 to-violet-500/10 px-4 py-2.5 text-sm font-semibold text-cyan-100 transition-all hover:border-cyan-300/60 disabled:opacity-50"
            >
              <RefreshCw className={`h-4 w-4 ${browseBusy ? "animate-spin" : "transition-transform group-hover:rotate-180"}`} />
              {browseBusy ? t("diskExplorer.scanning") : rootPath ? t("diskExplorer.rescan") : t("diskExplorer.scan")}
            </button>
          </div>
        </div>

        {browseBusy && (
          <div className="mt-4 flex items-center gap-3 text-xs text-slate-400">
            <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-slate-800">
              <div className="h-full w-1/3 animate-pulse rounded-full bg-gradient-to-r from-cyan-400 to-violet-400" />
            </div>
            <span className="font-mono">
              {t("diskExplorer.scannedNodes", { count: browseProgress?.scannedNodes ?? 0 })}
            </span>
          </div>
        )}
        {browseError && <div className="mt-4"><Notice tone="danger" message={browseError} /></div>}
      </section>

      {!rootPath && !browseBusy && (
        <div className="glass-panel flex flex-col items-center gap-2 rounded-2xl border border-cyan-500/10 p-10 text-center">
          <HardDrive className="h-8 w-8 text-cyan-300/60" />
          <h3 className="text-lg font-semibold text-slate-100">{t("diskExplorer.emptyTitle")}</h3>
          <p className="max-w-sm text-sm text-slate-400">{t("diskExplorer.emptyDescription")}</p>
        </div>
      )}

      {rootPath && (
        <section className="glass-panel cyber-glow p-6">
          <div className="flex flex-wrap items-center gap-1 pb-4 font-mono text-xs text-slate-400">
            {breadcrumbSegments(rootPath, currentPath).map((segment, index, all) => (
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

          {actionMessage && <div className="mb-4"><Notice tone="info" message={actionMessage} /></div>}

          {!browseBusy && children.length > 0 && (
            <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
              <div className="flex items-center gap-3">
                <button
                  onClick={goUp}
                  disabled={currentPath === rootPath}
                  className="inline-flex items-center gap-1.5 rounded-lg border border-cyan-500/20 bg-slate-950/50 px-2.5 py-1.5 text-xs font-medium text-cyan-200 transition hover:border-cyan-400/40 disabled:opacity-40"
                >
                  <ChevronRight className="h-3.5 w-3.5 rotate-180" />
                  {t("diskExplorer.up")}
                </button>
                <span className="text-sm text-slate-300">
                  {t("diskExplorer.folderTotal", { size: formatBytes(folderTotalBytes) })}
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
              <DiskTreemap items={treemapItems} onOpen={(item) => void openPath(item.path)} />

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
                {sortedChildren.map((item) => {
                  const permanent = item.sizeBytes >= DIRECT_DELETE_THRESHOLD_BYTES;
                  const deleting = deletingPaths.has(item.path);
                  const pending = pendingDeletePath === item.path;
                  return (
                  <div
                    key={item.path}
                    className={`flex flex-col gap-2 bg-slate-950/30 px-3 py-2.5 text-sm transition-all duration-200 ease-in ${
                      deleting
                        ? "-translate-x-2 scale-[0.98] opacity-0"
                        : pending
                          ? "translate-x-0 scale-100 opacity-60"
                          : "translate-x-0 scale-100 opacity-100"
                    }`}
                    style={deleting ? { maxHeight: 0, paddingTop: 0, paddingBottom: 0, overflow: "hidden" } : undefined}
                  >
                  <div className="flex items-center gap-3">
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
                      (pending ? (
                        <span className="flex shrink-0 items-center gap-1.5 rounded-md border border-rose-400/30 bg-rose-400/10 px-2 py-1 font-mono text-[10px] uppercase tracking-widest text-rose-200">
                          <RefreshCw className="h-3 w-3 animate-spin" />
                          {t("diskExplorer.deleting")}
                        </span>
                      ) : confirmingDeletePath === item.path ? (
                        <div className="flex shrink-0 items-center gap-1">
                          <button
                            onClick={() => void deleteItem(item)}
                            className="rounded-md border border-rose-400/40 bg-rose-400/15 px-2 py-1 font-mono text-[10px] uppercase tracking-widest text-rose-100 hover:bg-rose-400/25"
                          >
                            {permanent ? t("diskExplorer.confirmDeletePermanent") : t("diskExplorer.confirmDelete")}
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
                          title={permanent ? t("diskExplorer.deleteHintPermanent") : t("diskExplorer.deleteHint")}
                          className="shrink-0 rounded-md border border-white/10 bg-slate-950/60 p-1.5 text-slate-500 transition hover:border-rose-400/40 hover:text-rose-200"
                        >
                          <Trash2 className="h-3.5 w-3.5" />
                        </button>
                      ))}
                  </div>
                  {confirmingDeletePath === item.path && permanent && (
                    <div className="flex items-center gap-2 rounded-md border border-rose-400/30 bg-rose-400/10 px-2.5 py-1.5 text-xs text-rose-200">
                      <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
                      {t("diskExplorer.permanentWarning", { size: formatBytes(item.sizeBytes) })}
                    </div>
                  )}
                  </div>
                  );
                })}
              </div>
            </>
          )}
        </section>
      )}

      {actionError && (
        <ErrorDialog message={actionError} onClose={() => setActionError(null)} t={t} />
      )}
    </div>
  );
}

function ErrorDialog({ message, onClose, t }: { message: string; onClose: () => void; t: (key: string) => string }) {
  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-slate-950/75 px-4 backdrop-blur-sm">
      <section
        role="alertdialog"
        aria-modal="true"
        aria-label={t("diskExplorer.errorDialogTitle")}
        className="w-full max-w-md rounded-2xl border border-rose-400/30 bg-slate-950 p-6 shadow-[0_25px_80px_-30px_hsl(350_90%_55%/0.6)]"
      >
        <div className="flex items-center gap-2 font-mono text-[10px] uppercase tracking-[0.25em] text-rose-300">
          <AlertTriangle className="h-3.5 w-3.5" />
          {t("diskExplorer.errorDialogTitle")}
        </div>
        <p className="mt-3 text-sm leading-relaxed text-slate-200">{message}</p>
        <div className="mt-6 flex justify-end">
          <button
            onClick={onClose}
            className="rounded-xl border border-rose-400/40 bg-rose-400/10 px-4 py-2 text-sm font-semibold text-rose-100 transition hover:bg-rose-400/15"
          >
            {t("common.close")}
          </button>
        </div>
      </section>
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
