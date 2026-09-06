import { useEffect, useState } from "react";
import { AlertTriangle, ChevronRight, File, Folder, HardDrive, Lock, RefreshCw, Shield, Trash2, X } from "lucide-react";
import { useI18n } from "@/i18n";
import { useTelemetry } from "@/hooks/useTelemetry";
import {
  cancelDiskTreeScan,
  deleteDiskUsageItem,
  getDiskUsageSummary,
  isTauriRuntime,
  listDiskDirectory,
  listDiskVolumes,
  listenToDiskTreeItemReady,
  listenToDiskTreeProgress,
  type DiskTreeItemUpdate,
  type DiskTreeNodeSummary,
  type DiskTreeProgress,
  type DiskVolumeInfo,
} from "@/services/tauri/agent";
import type { DiskNearFullInfo } from "@/services/insights";
import { DiskTreemap, DiskTreemapLegend } from "@/components/analystblaze/DiskTreemap";

/** Mirrors disk_usage.rs's DIRECT_DELETE_THRESHOLD_BYTES - only used here
 * to warn before the click; the backend enforces the real behavior
 * regardless of what this shows. */
const DIRECT_DELETE_THRESHOLD_BYTES = 2 * 1024 * 1024 * 1024;
/** Per-drive threshold for the "disk near capacity" insight - deliberately
 * higher than the 80% whole-machine warning already shown elsewhere, since
 * this points at one specific drive rather than an aggregate. */
const DISK_NEAR_FULL_THRESHOLD_PERCENT = 90;
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

/** Picks the fullest volume over the threshold (if any) and, only then,
 * pays for the heavier categorized scan to find its top offenders - cheap
 * in the common case where nothing is near capacity, since listDiskVolumes()
 * was already fetched for the volume picker regardless. */
async function detectDiskNearFull(
  volumes: DiskVolumeInfo[],
  onDetected?: (info: DiskNearFullInfo | null) => void,
): Promise<void> {
  if (!onDetected) return;
  const candidates = volumes
    .filter((volume) => volume.totalBytes > 0)
    .map((volume) => ({
      volume,
      usedPercent: ((volume.totalBytes - volume.availableBytes) / volume.totalBytes) * 100,
    }))
    .filter((entry) => entry.usedPercent >= DISK_NEAR_FULL_THRESHOLD_PERCENT)
    .sort((a, b) => b.usedPercent - a.usedPercent);

  if (candidates.length === 0) {
    onDetected(null);
    return;
  }

  const { volume, usedPercent } = candidates[0];
  try {
    const summary = await getDiskUsageSummary();
    const mount = volume.mountPoint.toLowerCase();
    const offenders = summary.categories
      .flatMap((category) => category.items)
      // Cache-category items carry a cleanup-category id in `path`, not a
      // real filesystem path, so they can't be matched to a drive letter.
      .filter((item) => !item.deletesViaCleanupCategory && item.path.toLowerCase().startsWith(mount))
      .sort((a, b) => b.sizeBytes - a.sizeBytes)
      .slice(0, 5)
      .map((item) => ({ label: item.label, sizeBytes: item.sizeBytes }));

    onDetected({
      mountPoint: volume.mountPoint,
      label: volume.label,
      usedPercent,
      totalBytes: volume.totalBytes,
      topOffenders: offenders,
      computedAt: Date.now(),
    });
  } catch {
    // The volume is still genuinely near full even if the offender scan
    // fails - report it without offenders rather than staying silent.
    onDetected({
      mountPoint: volume.mountPoint,
      label: volume.label,
      usedPercent,
      totalBytes: volume.totalBytes,
      topOffenders: [],
      computedAt: Date.now(),
    });
  }
}

function parentPath(path: string): string | null {
  const trimmed = path.replace(/[\\/]+$/, "");
  const lastSep = Math.max(trimmed.lastIndexOf("\\"), trimmed.lastIndexOf("/"));
  // No separator left at all means `path` was already a bare root like "C:"
  // - nothing above that to navigate to.
  if (lastSep < 0) return null;
  // The separator sits right after the drive letter (e.g. "C:\Users"): the
  // parent IS the drive root itself, "C:\" - keep its trailing separator so
  // this matches rootPath's own stored format (openPath("C:\") is what
  // loadRoot uses to open a volume, and currentPath === rootPath is what
  // disables the Up button once there). Previously this branch returned
  // null instead, which silently broke "Up" for any top-level folder -
  // C:\Users, C:\Windows, C:\Program Files - the most common ones to browse.
  if (lastSep <= 2) return trimmed.slice(0, lastSep + 1);
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
  onDiskNearFullDetected,
}: {
  /** Set (transiently) when the user navigated here from an Insights card
   * asking to see disk-usage details - triggers loading the first detected
   * volume's root on arrival. */
  autoScan?: boolean;
  onAutoScanHandled?: () => void;
  /** Called at most once per volume list load, only when some drive is
   * actually near capacity - lets Insights show a card without this screen
   * needing to know anything about how insights are displayed. */
  onDiskNearFullDetected?: (info: DiskNearFullInfo | null) => void;
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
  // Directory children start at sizeBytes 0 (a real, already-empty folder
  // looks identical) - this is what actually distinguishes "still
  // calculating" from "genuinely empty" for the size column, seeded from
  // every directory in a fresh listing and cleared as each one's
  // DISK_TREE_ITEM_READY_EVENT arrives.
  const [pendingSizePaths, setPendingSizePaths] = useState<Set<string>>(new Set());
  const [browseBusy, setBrowseBusy] = useState(false);
  const [browseError, setBrowseError] = useState<string | null>(null);
  const [browseProgress, setBrowseProgress] = useState<DiskTreeProgress | null>(null);

  const [sortBy, setSortBy] = useState<"size" | "name">("size");
  const [confirmingDeletePath, setConfirmingDeletePath] = useState<string | null>(null);
  const [pendingDeletePath, setPendingDeletePath] = useState<string | null>(null);
  const [deletingPaths, setDeletingPaths] = useState<Set<string>>(new Set());
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  // Keyed by path, but the VALUE (not just membership) matters: once the
  // user navigates elsewhere, that folder's `children` are gone from state,
  // so this is the only place a selected item's name/size/isDir survive to
  // be shown in the final confirmation list - deliberately not a Set<string>
  // for that reason. Never cleared on navigation (see openPath) - marking
  // something for deletion in one folder and then browsing elsewhere is
  // exactly the flow this is for.
  const [selectedPaths, setSelectedPaths] = useState<Map<string, DiskTreeNodeSummary>>(new Map());
  const [confirmingBulkDelete, setConfirmingBulkDelete] = useState(false);
  const [bulkDeleteBusy, setBulkDeleteBusy] = useState(false);
  const [bulkProgress, setBulkProgress] = useState<{ done: number; total: number } | null>(null);

  useEffect(() => {
    if (!runtimeAvailable) return;
    listDiskVolumes()
      .then((list) => {
        setVolumes(list);
        setSelectedVolume((current) => current || list[0]?.mountPoint || "");
        void detectDiskNearFull(list, onDiskNearFullDetected);
      })
      .catch((error) => setVolumesError(errorMessage(error)));
    // Only re-run when the runtime becomes available - onDiskNearFullDetected
    // is expected to be a stable callback from AppShell, not something that
    // should re-trigger a fresh scan on every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [runtimeAvailable]);

  useEffect(() => {
    if (!runtimeAvailable) return;
    let dispose: (() => void) | undefined;
    listenToDiskTreeProgress((progress) => setBrowseProgress(progress)).then((next) => {
      dispose = next;
    });
    return () => dispose?.();
  }, [runtimeAvailable]);

  // Directory sizes arrive after the listing itself (see listDiskDirectory's
  // docs) - patch each row in place as its real size/protection status
  // lands instead of waiting for every subfolder to finish. Also patches
  // selectedPaths: it holds its own snapshot of each item, so a folder
  // selected while still "calculating" (sizeBytes 0) would otherwise stay
  // stuck at 0 in the bulk-delete total even after the real size arrives.
  useEffect(() => {
    if (!runtimeAvailable) return;
    let dispose: (() => void) | undefined;
    const applyUpdate = (update: DiskTreeItemUpdate) => {
      const patch = (item: DiskTreeNodeSummary): DiskTreeNodeSummary =>
        item.path === update.path
          ? { ...item, sizeBytes: update.sizeBytes, protected: update.protected, actionable: update.actionable }
          : item;
      setChildren((current) => current.map(patch));
      setSelectedPaths((current) => {
        if (!current.has(update.path)) return current;
        const next = new Map(current);
        next.set(update.path, patch(current.get(update.path)!));
        return next;
      });
      setPendingSizePaths((current) => {
        if (!current.has(update.path)) return current;
        const next = new Set(current);
        next.delete(update.path);
        return next;
      });
    };
    listenToDiskTreeItemReady(applyUpdate).then((next) => {
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
    // The previous folder's directory sizes may still be resolving in the
    // background (listDiskDirectory returns before that finishes) - stop
    // it before starting a new listing, otherwise navigating through
    // several folders quickly stacks up concurrent background scans
    // instead of replacing one with the next.
    try {
      await cancelDiskTreeScan();
    } catch {
      // Nothing was in flight, or the call itself failed - either way
      // this is best-effort, not worth blocking navigation over.
    }
    try {
      const kids = await listDiskDirectory(path);
      setCurrentPath(path);
      setChildren(kids);
      setPendingSizePaths(new Set(kids.filter((item) => item.isDir).map((item) => item.path)));
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

  const toggleSelected = (item: DiskTreeNodeSummary) => {
    setSelectedPaths((current) => {
      const next = new Map(current);
      if (next.has(item.path)) next.delete(item.path);
      else next.set(item.path, item);
      return next;
    });
  };

  const removeFromSelection = (path: string) => {
    setSelectedPaths((current) => {
      const next = new Map(current);
      next.delete(path);
      return next;
    });
  };

  const deleteSelected = async () => {
    setConfirmingBulkDelete(false);
    // From the persisted map, not sortedChildren - items marked in a folder
    // the user has since navigated away from are no longer part of
    // sortedChildren at all, and deleting only what's still visible in the
    // CURRENT folder would silently drop the rest of the selection.
    const items = Array.from(selectedPaths.values());
    if (items.length === 0) return;
    setBulkDeleteBusy(true);
    setActionMessage(null);
    setActionError(null);
    const failures: string[] = [];
    let successCount = 0;
    for (let index = 0; index < items.length; index += 1) {
      const item = items[index];
      setBulkProgress({ done: index, total: items.length });
      setPendingDeletePath(item.path);
      try {
        const result = await deleteDiskUsageItem(item.path);
        const outcome = result as { success?: boolean; message?: string } | undefined;
        if (outcome && outcome.success === false) {
          failures.push(`${item.name}: ${outcome.message ?? ""}`);
          continue;
        }
        successCount += 1;
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
        failures.push(`${item.name}: ${errorMessage(error)}`);
      }
    }
    setPendingDeletePath(null);
    setBulkProgress(null);
    setSelectedPaths(new Map());
    setBulkDeleteBusy(false);
    track("disk_tree_bulk_deleted", { count: successCount, failed: failures.length });
    if (successCount > 0) {
      setActionMessage(t("diskExplorer.bulkDeleteSuccess", { count: successCount }));
    }
    if (failures.length > 0) {
      setActionError(failures.join("\n"));
    }
  };

  const sortedChildren = [...children].sort((a, b) =>
    sortBy === "size" ? b.sizeBytes - a.sizeBytes : a.name.localeCompare(b.name),
  );
  const treemapItems = sortedChildren.filter((item) => !deletingPaths.has(item.path));
  const folderTotalBytes = children.reduce((sum, item) => sum + item.sizeBytes, 0);
  const selectableChildren = sortedChildren.filter((item) => item.actionable && !deletingPaths.has(item.path));
  const allSelected = selectableChildren.length > 0 && selectableChildren.every((item) => selectedPaths.has(item.path));
  const toggleSelectAll = () => {
    // Adds/removes only the CURRENT folder's items - selections made in
    // other folders (no longer part of selectableChildren) must survive
    // this, same as they survive plain navigation.
    setSelectedPaths((current) => {
      const next = new Map(current);
      for (const item of selectableChildren) {
        if (allSelected) next.delete(item.path);
        else next.set(item.path, item);
      }
      return next;
    });
  };
  // From the persisted map (spans every folder visited this session), not
  // sortedChildren (only the current folder) - see the state's own comment.
  const selectedItems = Array.from(selectedPaths.values());
  // Folder/file/permanent breakdown for the sticky bar's summary text moved
  // into BulkDeleteConfirmDialog itself, computed from live `items` there -
  // only selectedTotalBytes is still needed here, for the sticky bar shown
  // while still browsing (before the confirm dialog opens at all).
  const selectedTotalBytes = selectedItems.reduce((sum, item) => sum + item.sizeBytes, 0);

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
                <div className="flex items-center gap-2">
                  <input
                    type="checkbox"
                    checked={allSelected}
                    onChange={toggleSelectAll}
                    disabled={bulkDeleteBusy || selectableChildren.length === 0}
                    aria-label={t("diskExplorer.selectAll")}
                    className="h-4 w-4 accent-cyan-400"
                  />
                  <h3 className="font-mono text-[10px] uppercase tracking-widest text-slate-500">
                    {t("diskExplorer.listTitle")}
                  </h3>
                </div>
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

              {selectedPaths.size > 0 && (
                <div className="sticky top-0 z-10 mt-3 flex flex-wrap items-center justify-between gap-3 rounded-xl border-2 border-rose-400/50 bg-rose-500/15 px-4 py-3 shadow-[0_10px_40px_-15px_hsl(350_90%_55%/0.6)] backdrop-blur">
                  <span className="flex items-center gap-2 text-sm font-semibold text-rose-100">
                    <Trash2 className="h-4 w-4 shrink-0" />
                    {bulkProgress
                      ? t("diskExplorer.bulkDeleting", { done: bulkProgress.done + 1, total: bulkProgress.total })
                      : t("diskExplorer.selectedSummary", { count: selectedItems.length, size: formatBytes(selectedTotalBytes) })}
                  </span>
                  <div className="flex items-center gap-2">
                    <button
                      disabled={bulkDeleteBusy}
                      onClick={() => setSelectedPaths(new Map())}
                      className="rounded-lg border border-white/15 bg-slate-950/40 px-3 py-1.5 text-xs font-medium text-slate-200 transition hover:text-white disabled:opacity-40"
                    >
                      {t("diskExplorer.clearSelection")}
                    </button>
                    <button
                      disabled={bulkDeleteBusy}
                      onClick={() => setConfirmingBulkDelete(true)}
                      className="inline-flex items-center gap-1.5 rounded-lg border border-rose-300/60 bg-rose-500 px-3.5 py-1.5 text-xs font-semibold text-white transition hover:bg-rose-400 disabled:opacity-50"
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                      {t("diskExplorer.deleteSelected")}
                    </button>
                  </div>
                </div>
              )}

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
                    {item.actionable ? (
                      <input
                        type="checkbox"
                        checked={selectedPaths.has(item.path)}
                        onChange={() => toggleSelected(item)}
                        disabled={bulkDeleteBusy || pending || deleting}
                        aria-label={t("diskExplorer.selectItem", { name: item.name })}
                        className="h-4 w-4 shrink-0 accent-cyan-400"
                      />
                    ) : (
                      <span className="h-4 w-4 shrink-0" />
                    )}
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
                    <span className="flex w-20 shrink-0 items-center justify-end gap-1 text-right font-mono text-xs text-slate-400">
                      {pendingSizePaths.has(item.path) ? (
                        <RefreshCw className="h-3 w-3 shrink-0 animate-spin text-slate-500" />
                      ) : (
                        formatBytes(item.sizeBytes)
                      )}
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
                          disabled={bulkDeleteBusy}
                          onClick={() => setConfirmingDeletePath(item.path)}
                          title={permanent ? t("diskExplorer.deleteHintPermanent") : t("diskExplorer.deleteHint")}
                          className="shrink-0 rounded-md border border-white/10 bg-slate-950/60 p-1.5 text-slate-500 transition hover:border-rose-400/40 hover:text-rose-200 disabled:opacity-40"
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

      {confirmingBulkDelete && (
        <BulkDeleteConfirmDialog
          items={selectedItems}
          onRemoveItem={removeFromSelection}
          onConfirm={() => void deleteSelected()}
          onCancel={() => setConfirmingBulkDelete(false)}
          t={t}
        />
      )}

      {actionError && (
        <ErrorDialog message={actionError} onClose={() => setActionError(null)} t={t} />
      )}
    </div>
  );
}

/** Shown right before a bulk delete actually runs - the one place the user
 * sees everything they've marked at once, since items can now come from
 * several different folders visited over the course of browsing (see
 * DiskExplorer's selectedPaths comment). Lets them drop individual items
 * from here too, in case reviewing the full list changes their mind about
 * one of them, without having to go find it again in its original folder. */
function BulkDeleteConfirmDialog({
  items,
  onRemoveItem,
  onConfirm,
  onCancel,
  t,
}: {
  items: DiskTreeNodeSummary[];
  onRemoveItem: (path: string) => void;
  onConfirm: () => void;
  onCancel: () => void;
  t: (key: string, params?: Record<string, string | number>) => string;
}) {
  // Removing the last item leaves nothing to confirm - close rather than
  // leave an empty dialog with a confirm button that would have nothing to do.
  useEffect(() => {
    if (items.length === 0) onCancel();
  }, [items.length, onCancel]);

  if (items.length === 0) return null;

  const folderCount = items.filter((item) => item.isDir).length;
  const fileCount = items.length - folderCount;
  const totalBytes = items.reduce((sum, item) => sum + item.sizeBytes, 0);
  const permanentCount = items.filter((item) => item.sizeBytes >= DIRECT_DELETE_THRESHOLD_BYTES).length;

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-slate-950/75 px-4 backdrop-blur-sm">
      <section
        role="alertdialog"
        aria-modal="true"
        aria-label={t("diskExplorer.bulkConfirmTitle", { count: items.length })}
        className="flex w-full max-w-lg flex-col rounded-2xl border border-rose-400/30 bg-slate-950 p-6 shadow-[0_25px_80px_-30px_hsl(350_90%_55%/0.6)]"
      >
        <div className="flex items-center gap-2 font-mono text-[10px] uppercase tracking-[0.25em] text-rose-300">
          <AlertTriangle className="h-3.5 w-3.5" />
          {t("diskExplorer.bulkConfirmTitle", { count: items.length })}
        </div>
        <p className="mt-3 text-sm leading-relaxed text-slate-200">
          {t("diskExplorer.bulkConfirmSummary", { folders: folderCount, files: fileCount, size: formatBytes(totalBytes) })}
        </p>
        {permanentCount > 0 && (
          <div className="mt-3 flex items-center gap-2 rounded-md border border-rose-400/30 bg-rose-400/10 px-2.5 py-1.5 text-xs text-rose-200">
            <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
            {t("diskExplorer.bulkPermanentWarning", { count: permanentCount })}
          </div>
        )}

        <ul className="mt-4 max-h-64 overflow-y-auto rounded-xl border border-white/10 bg-slate-900/40">
          {items.map((item) => {
            const permanent = item.sizeBytes >= DIRECT_DELETE_THRESHOLD_BYTES;
            const parent = parentPath(item.path);
            return (
              <li
                key={item.path}
                className="flex items-center gap-2.5 border-b border-white/5 px-3 py-2 text-sm last:border-b-0"
              >
                {item.isDir ? (
                  <Folder className="h-3.5 w-3.5 shrink-0 text-cyan-300" />
                ) : (
                  <File className="h-3.5 w-3.5 shrink-0 text-violet-300" />
                )}
                <div className="min-w-0 flex-1">
                  <div className="truncate text-slate-100" title={item.path}>
                    {item.name}
                  </div>
                  {parent && <div className="truncate font-mono text-[10px] text-slate-500">{parent}</div>}
                </div>
                {permanent && <AlertTriangle className="h-3.5 w-3.5 shrink-0 text-rose-300" />}
                <span className="w-16 shrink-0 text-right font-mono text-xs text-slate-400">
                  {formatBytes(item.sizeBytes)}
                </span>
                <button
                  onClick={() => onRemoveItem(item.path)}
                  title={t("diskExplorer.removeFromSelection")}
                  className="shrink-0 rounded-md p-1 text-slate-500 transition hover:text-rose-200"
                >
                  <X className="h-3.5 w-3.5" />
                </button>
              </li>
            );
          })}
        </ul>

        <div className="mt-6 flex justify-end gap-2">
          <button
            onClick={onCancel}
            className="rounded-xl border border-slate-600/40 px-4 py-2 text-sm font-medium text-slate-300 transition hover:text-slate-100"
          >
            {t("common.cancel")}
          </button>
          <button
            onClick={onConfirm}
            className="rounded-xl border border-rose-400/40 bg-rose-400/15 px-4 py-2 text-sm font-semibold text-rose-100 transition hover:bg-rose-400/25"
          >
            {t("diskExplorer.deleteSelected")}
          </button>
        </div>
      </section>
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
