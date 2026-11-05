import { useEffect, useRef, useState } from "react";
import { AlertTriangle, ArrowLeft, ChevronRight, File, Folder, HardDrive, Lock, RefreshCw, Shield, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { ConfirmInline } from "@/components/ui/confirm-inline";
import { EmptyState } from "@/components/ui/empty-state";
import { Progress } from "@/components/ui/progress";
import { Segmented } from "@/components/ui/segmented";
import { StatusPill } from "@/components/ui/status-pill";
import { useI18n } from "@/i18n";
import { useTelemetry } from "@/hooks/useTelemetry";
import { useDiskVolumes } from "@/features/disk/useDiskVolumes";
import { useCleanup } from "@/features/cleanup/useCleanup";
import {
  cancelDiskTreeScan,
  checkDiskOptimizationInsight,
  deleteDiskUsageItem,
  getDiskUsageSummary,
  isTauriRuntime,
  listDiskDirectory,
  listenToDiskTreeItemReady,
  listenToDiskTreeProgress,
  type DiskOptimizationInsight,
  type DiskTreeItemUpdate,
  type DiskTreeNodeSummary,
  type DiskTreeProgress,
  type DiskVolumeInfo,
} from "@/services/tauri/agent";
import type { DiskNearFullInfo } from "@/services/insights";
import { formatBytes } from "@/lib/format";
import { cn } from "@/lib/utils";
import { DiskTreemap, DiskTreemapLegend } from "./DiskTreemap";

/**
 * Espelha `DIRECT_DELETE_THRESHOLD_BYTES` de
 * src-tauri/src/optimizations/disk_usage.rs (2 GiB). O agente não expõe o
 * valor, então ele mora só aqui: serve para avisar ANTES do clique e entra
 * nos textos via formatBytes (nunca "2 GB" fixo). Quem decide de verdade é
 * o backend, seja qual for o aviso mostrado. Se a constante do Rust mudar,
 * mude esta junto.
 */
const DIRECT_DELETE_THRESHOLD_BYTES = 2 * 1024 * 1024 * 1024;
/** Limite por unidade para o insight "disco quase cheio" - de propósito
 * acima do aviso de 80% da máquina inteira mostrado em outros lugares. */
const DISK_NEAR_FULL_THRESHOLD_PERCENT = 90;
/** Duração do fade/colapso antes de a linha sair da lista - igual à
 * transição CSS da linha (duration-200 + folga). */
const DELETE_ANIMATION_MS = 260;
/** Categoria de limpeza que é a própria quarentena: não entra no atalho. */
const CLEANUP_QUARANTINE_ID = "cleanup_quarantine";

type Translate = (key: string, params?: Record<string, string | number>) => string;

function errorMessage(error: unknown) {
  if (error instanceof Error) return error.message;
  return String(error);
}

function isPermanent(item: DiskTreeNodeSummary) {
  return item.sizeBytes >= DIRECT_DELETE_THRESHOLD_BYTES;
}

function volumeName(volume: DiskVolumeInfo) {
  const drive = volume.mountPoint.replace(/[\\/]+$/, "");
  const label = volume.label?.trim();
  return label && label !== drive && label !== volume.mountPoint ? `${drive} · ${label}` : drive;
}

function usedPercentOf(volume: DiskVolumeInfo) {
  return volume.totalBytes > 0 ? ((volume.totalBytes - volume.availableBytes) / volume.totalBytes) * 100 : 0;
}

/** Escolhe a unidade mais cheia acima do limite e só então paga a leitura
 * por categorias para achar os maiores itens - barato no caso comum, em que
 * nada está quase cheio. */
async function detectDiskNearFull(
  volumes: DiskVolumeInfo[],
  onDetected?: (info: DiskNearFullInfo | null) => void,
): Promise<void> {
  if (!onDetected) return;
  const candidates = volumes
    .filter((volume) => volume.totalBytes > 0)
    .map((volume) => ({ volume, usedPercent: usedPercentOf(volume) }))
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
      // Itens de cache trazem o id da categoria de limpeza em `path`, não
      // um caminho real: não dá para casar com a letra da unidade.
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
    // A unidade continua quase cheia mesmo se a leitura dos maiores itens
    // falhar - avisa sem eles em vez de ficar calado.
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
  // Sem separador: já é a raiz ("C:").
  if (lastSep < 0) return null;
  // Separador logo após a letra (ex.: "C:\Users"): o pai é "C:\", com a
  // barra, no mesmo formato de rootPath (senão "Subir" quebra nas pastas
  // de topo).
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

/**
 * Meu PC › Disco (board Tela-MeuPC-Disco). Mesmo comportamento do
 * DiskExplorer antigo com a pele D3: só a pasta aberta é calculada,
 * exclusão por linha com confirmação na própria linha, seleção que
 * atravessa pastas e confirmação em lote.
 */
export function Disk({
  autoScan,
  onAutoScanHandled,
  onDiskNearFullDetected,
  onOpenCleanup,
  shadowConsentNeeded,
  onResolveShadowConsent,
}: {
  /** Ligado (por um instante) quando o usuário veio de um card de Insights
   * pedindo os detalhes do disco - abre a raiz da primeira unidade. */
  autoScan?: boolean;
  onAutoScanHandled?: () => void;
  /** Chamado no máximo uma vez por visita, quando alguma unidade está quase
   * cheia - deixa Insights mostrar um card sem esta tela saber como. */
  onDiskNearFullDetected?: (info: DiskNearFullInfo | null) => void;
  /** Abre Controles › Limpeza (doc 14 U3). Sem ela, o atalho mostra só o valor. */
  onOpenCleanup?: () => void;
  /** Card de Shadow Storage (opcional: só aparece se o shell passar as duas). */
  shadowConsentNeeded?: boolean;
  onResolveShadowConsent?: (choice: "auto" | "manual") => Promise<void>;
}) {
  const { t } = useI18n();
  const track = useTelemetry("disk_explorer");
  const runtimeAvailable = isTauriRuntime();

  const { volumes, loading: volumesLoading, error: volumesError, refresh: refreshVolumes } = useDiskVolumes({
    auto: runtimeAvailable,
  });
  const cleanup = useCleanup({ auto: runtimeAvailable });
  const cleanupCategories = cleanup.categories.filter((category) => category.id !== CLEANUP_QUARANTINE_ID);
  const cleanupBytes = cleanupCategories.reduce((sum, category) => sum + category.reclaimableBytes, 0);

  const [selectedVolume, setSelectedVolume] = useState<string>("");
  const activeVolume = selectedVolume || volumes[0]?.mountPoint || "";
  // Só não é null quando o disco de boot é HD E a otimização agendada está
  // desligada (ver checkDiskOptimizationInsight). Lido uma vez por visita.
  const [diskOptimizationInsight, setDiskOptimizationInsight] = useState<DiskOptimizationInsight | null>(null);
  const [shadowBusy, setShadowBusy] = useState(false);

  const [rootPath, setRootPath] = useState<string | null>(null);
  const [currentPath, setCurrentPath] = useState<string>("");
  // Nada fica guardado no backend: cada navegação pede de novo só os filhos
  // imediatos desta pasta.
  const [children, setChildren] = useState<DiskTreeNodeSummary[]>([]);
  // Pastas começam com sizeBytes 0 (igual a uma pasta vazia de verdade):
  // é este conjunto que separa "ainda calculando" de "vazia".
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

  // Por caminho, guardando o ITEM (nome/tamanho/isDir): depois de navegar,
  // os filhos da pasta anterior somem do estado e este mapa é o único lugar
  // de onde a confirmação final tira esses dados. Nunca é limpo ao navegar.
  const [selectedPaths, setSelectedPaths] = useState<Map<string, DiskTreeNodeSummary>>(new Map());
  const [confirmingBulkDelete, setConfirmingBulkDelete] = useState(false);
  const [bulkDeleteBusy, setBulkDeleteBusy] = useState(false);
  const [bulkProgress, setBulkProgress] = useState<{ done: number; total: number } | null>(null);

  // Detecção de "disco quase cheio": uma vez por visita, na primeira lista
  // de unidades que chegar (onDiskNearFullDetected vem estável do shell).
  const nearFullChecked = useRef(false);
  useEffect(() => {
    if (!runtimeAvailable || nearFullChecked.current) return;
    if (volumes.length === 0) return;
    nearFullChecked.current = true;
    void detectDiskNearFull(volumes, onDiskNearFullDetected);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [runtimeAvailable, volumes]);

  useEffect(() => {
    if (!runtimeAvailable) return;
    checkDiskOptimizationInsight()
      .then((insight) => setDiskOptimizationInsight(insight))
      .catch(() => undefined);
  }, [runtimeAvailable]);

  useEffect(() => {
    if (!runtimeAvailable) return;
    let dispose: (() => void) | undefined;
    listenToDiskTreeProgress((progress) => setBrowseProgress(progress)).then((next) => {
      dispose = next;
    });
    return () => dispose?.();
  }, [runtimeAvailable]);

  // Tamanhos das pastas chegam depois da listagem: corrige cada linha no
  // lugar e também a seleção (que guarda a própria cópia do item; senão uma
  // pasta marcada ainda "calculando" ficaria com 0 no total do lote).
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
    void loadRoot(activeVolume || volumes[0].mountPoint);
    onAutoScanHandled?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [autoScan, volumes]);

  // Cancela a listagem em andamento ao sair da aba - uma pasta lenta não
  // pode deixar trabalho rodando numa tela que o usuário já deixou.
  useEffect(() => {
    return () => {
      void cancelDiskTreeScan().catch(() => undefined);
    };
  }, []);

  const openPath = async (path: string) => {
    setBrowseBusy(true);
    setBrowseError(null);
    setBrowseProgress(null);
    setConfirmingDeletePath(null);
    // Para o cálculo de tamanhos da pasta anterior antes de abrir outra,
    // senão navegar rápido empilha varreduras em paralelo.
    try {
      await cancelDiskTreeScan();
    } catch {
      // Nada em andamento, ou a chamada falhou: não vale travar a navegação.
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
    const volume = volumeOverride ?? activeVolume;
    if (!volume) return;
    setActionMessage(null);
    setRootPath(volume);
    track("disk_tree_root_opened", { volume });
    await openPath(volume);
  };

  const refreshFolder = () => {
    void refreshVolumes().catch(() => undefined);
    if (rootPath && currentPath && rootPath === activeVolume) {
      setActionMessage(null);
      void openPath(currentPath);
    } else {
      void loadRoot();
    }
  };

  const selectVolume = (mountPoint: string) => {
    setSelectedVolume(mountPoint);
    void loadRoot(mountPoint);
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

  const dropFromList = (path: string) => {
    // Toca o fade/colapso primeiro e só depois tira da lista: o corte seco
    // parecia "ainda está aí".
    setDeletingPaths((current) => new Set(current).add(path));
    window.setTimeout(() => {
      setChildren((current) => current.filter((child) => child.path !== path));
      setDeletingPaths((current) => {
        const next = new Set(current);
        next.delete(path);
        return next;
      });
    }, DELETE_ANIMATION_MS);
  };

  const deleteItem = async (item: DiskTreeNodeSummary) => {
    setConfirmingDeletePath(null);
    setActionMessage(null);
    setActionError(null);
    // Apagar uma pasta enorme pode demorar: a linha mostra "Apagando..." já.
    setPendingDeletePath(item.path);
    try {
      const result = await deleteDiskUsageItem(item.path);
      const outcome = result as { success?: boolean; message?: string } | undefined;
      if (outcome && outcome.success === false && outcome.message) {
        // Erro vai para a janela com o motivo, não para uma faixa que passa batido.
        setActionError(outcome.message);
        return;
      }
      // A mensagem do backend diz com precisão se foi para a quarentena ou
      // se foi apagado de vez - nunca trocar por um texto fixo.
      setActionMessage(outcome?.message ?? t("mypc.disk.deleteSuccess", { name: item.name }));
      track("disk_tree_item_deleted", { isDir: item.isDir, permanent: isPermanent(item) });
      setSelectedPaths((current) => {
        if (!current.has(item.path)) return current;
        const next = new Map(current);
        next.delete(item.path);
        return next;
      });
      dropFromList(item.path);
      void refreshVolumes().catch(() => undefined);
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
    // Do mapa persistido, não da pasta atual: itens marcados em pastas que
    // o usuário já deixou também fazem parte do lote.
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
        dropFromList(item.path);
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
      setActionMessage(t("mypc.disk.bulkDeleteSuccess", { count: successCount }));
      void refreshVolumes().catch(() => undefined);
    }
    if (failures.length > 0) setActionError(failures.join("\n"));
  };

  const resolveShadow = async (choice: "auto" | "manual") => {
    if (!onResolveShadowConsent) return;
    setShadowBusy(true);
    try {
      await onResolveShadowConsent(choice);
    } finally {
      setShadowBusy(false);
    }
  };

  const sortedChildren = [...children].sort((a, b) =>
    sortBy === "size" ? b.sizeBytes - a.sizeBytes : a.name.localeCompare(b.name),
  );
  const treemapItems = sortedChildren.filter((item) => !deletingPaths.has(item.path));
  const folderTotalBytes = children.reduce((sum, item) => sum + item.sizeBytes, 0);
  const totalDirCount = children.filter((item) => item.isDir).length;
  const doneDirCount = totalDirCount - pendingSizePaths.size;
  const selectableChildren = sortedChildren.filter((item) => item.actionable && !deletingPaths.has(item.path));
  const allSelected = selectableChildren.length > 0 && selectableChildren.every((item) => selectedPaths.has(item.path));
  const toggleSelectAll = () => {
    // Mexe só nos itens da pasta ATUAL - o que foi marcado em outras pastas fica.
    setSelectedPaths((current) => {
      const next = new Map(current);
      for (const item of selectableChildren) {
        if (allSelected) next.delete(item.path);
        else next.set(item.path, item);
      }
      return next;
    });
  };
  const selectedItems = Array.from(selectedPaths.values());
  const selectedTotalBytes = selectedItems.reduce((sum, item) => sum + item.sizeBytes, 0);
  const selectionHasPermanent = selectedItems.some(isPermanent);
  const limitText = formatBytes(DIRECT_DELETE_THRESHOLD_BYTES);

  if (!runtimeAvailable) {
    return (
      <EmptyState
        icon={<HardDrive />}
        title={t("mypc.disk.desktopOnlyTitle")}
        reason={t("mypc.disk.desktopOnlyReason")}
      />
    );
  }

  return (
    <div className="flex flex-col gap-5">
      {diskOptimizationInsight?.isHdd && diskOptimizationInsight.defrag?.enabled === false && (
        <div className="flex flex-col gap-3 rounded-card border border-warn/40 bg-warn/5 p-4 sm:flex-row sm:items-start sm:justify-between">
          <div className="flex min-w-0 items-start gap-3">
            <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warn" aria-hidden />
            <div className="min-w-0">
              <div className="text-sm font-semibold text-fg">{t("mypc.disk.defragTitle")}</div>
              <p className="mt-1 text-[13px] leading-relaxed text-fg-2">{t("mypc.disk.defragDesc")}</p>
              <p className="mt-1 text-[11px] text-fg-4">{t("mypc.disk.defragTech")}</p>
            </div>
          </div>
          <Button size="sm" variant="outline" onClick={() => setDiskOptimizationInsight(null)} className="self-end sm:self-auto">
            {t("common.close")}
          </Button>
        </div>
      )}

      {shadowConsentNeeded && onResolveShadowConsent && (
        <div className="flex flex-col gap-3 rounded-card border border-warn/40 bg-warn/5 p-4">
          <div className="flex min-w-0 items-start gap-3">
            <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warn" aria-hidden />
            <div className="min-w-0">
              <div className="text-sm font-semibold text-fg">{t("mypc.disk.shadowTitle")}</div>
              <p className="mt-1 text-[13px] leading-relaxed text-fg-2">{t("mypc.disk.shadowBody")}</p>
              <p className="mt-1 text-[11px] text-fg-4">{t("mypc.disk.shadowTech")}</p>
            </div>
          </div>
          <div className="flex flex-wrap justify-end gap-2">
            <Button size="sm" variant="outline" disabled={shadowBusy} onClick={() => void resolveShadow("manual")}>
              {t("mypc.disk.shadowManual")}
            </Button>
            <Button size="sm" variant="outline" disabled={shadowBusy} onClick={() => void resolveShadow("auto")}>
              {t("mypc.disk.shadowAuto")}
            </Button>
          </div>
        </div>
      )}

      {/* Unidades + promessa + Atualizar pasta */}
      <div className="flex flex-col gap-4 lg:flex-row lg:items-center">
        <div role="group" aria-label={t("mypc.disk.volumesLabel")} className="flex flex-wrap gap-3">
          {volumes.map((volume) => {
            const usedBytes = Math.max(0, volume.totalBytes - volume.availableBytes);
            const usedPercent = usedPercentOf(volume);
            const nearFull = usedPercent >= DISK_NEAR_FULL_THRESHOLD_PERCENT;
            const selected = volume.mountPoint === activeVolume;
            const name = volumeName(volume);
            const usedText = t("mypc.disk.volumeUsed", { used: formatBytes(usedBytes), total: formatBytes(volume.totalBytes) });
            return (
              <button
                key={volume.mountPoint}
                type="button"
                onClick={() => selectVolume(volume.mountPoint)}
                disabled={browseBusy || bulkDeleteBusy}
                aria-pressed={selected}
                aria-label={t("mypc.disk.volumeOpen", { name })}
                className={cn(
                  "flex w-[200px] flex-col gap-2 rounded-card border px-3.5 py-3 text-left transition-colors duration-fast ease-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 disabled:cursor-not-allowed disabled:opacity-60",
                  selected ? "border-accent/60 bg-accent/5" : "border-line bg-surface hover:border-fg-4/60",
                )}
              >
                <span className="flex items-baseline justify-between gap-2">
                  <span className="truncate text-[13px] font-semibold text-fg">{name}</span>
                  <span className={cn("shrink-0 text-[13px] tabular-nums", nearFull ? "text-warn" : "text-ok")}>
                    {t("mypc.disk.volumeFree", { free: formatBytes(volume.availableBytes) })}
                  </span>
                </span>
                {volume.totalBytes > 0 && (
                  <Progress value={usedPercent} label={usedText} tone={nearFull ? "warn" : "accent"} />
                )}
                <span className="truncate text-[11px] text-fg-3">
                  {usedText}
                  {volume.fileSystem ? ` · ${volume.fileSystem}` : ""}
                </span>
              </button>
            );
          })}
          {volumes.length === 0 && (
            <EmptyState
              variant="inline"
              title={
                volumesError
                  ? t("mypc.disk.volumesError")
                  : volumesLoading
                    ? t("mypc.disk.volumesLoading")
                    : t("mypc.disk.volumesEmpty")
              }
              reason={volumesError ?? undefined}
              action={
                volumesError ? (
                  <Button size="sm" variant="outline" onClick={() => void refreshVolumes().catch(() => undefined)}>
                    {t("mypc.disk.refreshFolder")}
                  </Button>
                ) : undefined
              }
            />
          )}
        </div>

        <p className="max-w-[440px] flex-1 text-[13px] leading-relaxed text-fg-3">{t("mypc.disk.promise")}</p>

        {rootPath && (
          <div className="flex items-center gap-2 lg:ml-auto">
            {browseBusy && (
              <Button size="sm" variant="outline" onClick={() => void cancelBrowse()}>
                <X aria-hidden />
                {t("mypc.disk.stop")}
              </Button>
            )}
            <Button size="sm" variant="outline" disabled={browseBusy || bulkDeleteBusy || !activeVolume} onClick={refreshFolder}>
              {t("mypc.disk.refreshFolder")}
            </Button>
          </div>
        )}
      </div>

      {/* Atalho para a Limpeza (doc 14 U3): temporários não aparecem navegando pastas. */}
      {!cleanup.error && (cleanup.loading || cleanup.categories.length > 0) && (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-control border border-line-soft bg-surface px-4 py-2.5 text-[13px] text-fg-2">
          <Trash2 className="size-4 shrink-0 text-fg-3" aria-hidden />
          <span className="tabular-nums">
            {cleanup.loading && cleanup.categories.length === 0
              ? t("mypc.disk.cleanupShortcutLoading")
              : t("mypc.disk.cleanupShortcut", { size: formatBytes(cleanupBytes) })}
          </span>
          {onOpenCleanup && (
            <Button variant="link" className="text-[13px]" onClick={onOpenCleanup}>
              {t("mypc.disk.cleanupShortcutAction")}
            </Button>
          )}
        </div>
      )}

      {!rootPath && !browseBusy && (
        <EmptyState
          icon={<HardDrive />}
          title={t("mypc.disk.emptyTitle")}
          reason={t("mypc.disk.emptyReason")}
          action={
            <Button variant="primary" disabled={!activeVolume} onClick={() => void loadRoot()}>
              {t("mypc.disk.openVolume")}
            </Button>
          }
        />
      )}

      {rootPath && (
        <Card padding="md" className="flex flex-col gap-4">
          {/* Subir · caminho · total · progresso real */}
          <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
            <Button
              variant="link"
              className="text-[13px]"
              onClick={goUp}
              disabled={browseBusy || currentPath === rootPath}
            >
              <ArrowLeft aria-hidden />
              {t("mypc.disk.up")}
            </Button>
            <nav aria-label={t("mypc.disk.breadcrumbLabel")} className="flex min-w-0 flex-wrap items-center gap-0.5 text-[13px] text-fg-3">
              {breadcrumbSegments(rootPath, currentPath).map((segment, index, all) => {
                const isCurrent = segment.path === currentPath;
                return (
                  <span key={segment.path} className="flex items-center gap-0.5">
                    <button
                      type="button"
                      onClick={() => void openPath(segment.path)}
                      disabled={isCurrent || browseBusy}
                      aria-current={isCurrent ? "location" : undefined}
                      className={cn(
                        "rounded-sm px-1 py-0.5 transition-colors duration-fast focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60",
                        isCurrent ? "cursor-default text-fg-2" : "hover:text-fg",
                      )}
                    >
                      {segment.label}
                    </button>
                    {index < all.length - 1 && <ChevronRight className="size-3 text-fg-4" aria-hidden />}
                  </span>
                );
              })}
            </nav>
            {!browseBusy && children.length > 0 && (
              <span className="text-[15px] font-semibold tabular-nums text-fg">
                {t("mypc.disk.folderTotal", { size: formatBytes(folderTotalBytes) })}
              </span>
            )}
            <div className="ml-auto flex items-center gap-3 text-xs text-fg-3">
              {browseBusy ? (
                <span className="flex items-center gap-2">
                  <RefreshCw className="size-3.5 animate-spin" aria-hidden />
                  {t("mypc.disk.listing")}
                  {browseProgress && browseProgress.scannedNodes > 0 && (
                    <span className="tabular-nums">
                      · {t("mypc.disk.scannedNodes", { count: browseProgress.scannedNodes })}
                    </span>
                  )}
                </span>
              ) : (
                pendingSizePaths.size > 0 &&
                totalDirCount > 0 && (
                  <>
                    <span className="tabular-nums">
                      {t("mypc.disk.calculating", { done: doneDirCount, total: totalDirCount })}
                    </span>
                    <Progress
                      className="w-24"
                      value={(doneDirCount / totalDirCount) * 100}
                      label={t("mypc.disk.calculatingLabel", { done: doneDirCount, total: totalDirCount })}
                    />
                  </>
                )
              )}
            </div>
          </div>

          {actionMessage && <p className="text-[13px] text-ok" role="status">{actionMessage}</p>}
          {browseError && <p className="text-[13px] text-danger" role="alert">{browseError}</p>}

          {browseBusy ? (
            <EmptyState variant="inline" title={t("mypc.disk.listing")} className="py-6" />
          ) : sortedChildren.length === 0 ? (
            !browseError && (
              <EmptyState icon={<Folder />} title={t("mypc.disk.folderEmpty")} reason={t("mypc.disk.folderEmptyReason")} />
            )
          ) : (
            <div className="grid gap-5 lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)]">
              <div className="flex flex-col gap-3">
                <DiskTreemap
                  items={treemapItems}
                  onOpen={(item) => void openPath(item.path)}
                  label={t("mypc.disk.treemapLabel")}
                  lockedHint={t("mypc.disk.treemapLockedHint")}
                />
                <DiskTreemapLegend
                  folders={t("mypc.disk.legend.folders")}
                  files={t("mypc.disk.legend.files")}
                  locked={t("mypc.disk.legend.locked")}
                />
              </div>

              <div className="flex min-w-0 flex-col">
                <div className="flex items-center justify-between gap-3 border-b border-line-soft pb-3">
                  <label className="flex cursor-pointer items-center gap-3 text-[13px] text-fg-3">
                    <input
                      type="checkbox"
                      checked={allSelected}
                      onChange={toggleSelectAll}
                      disabled={bulkDeleteBusy || selectableChildren.length === 0}
                      className="size-4 shrink-0 accent-accent disabled:cursor-not-allowed"
                    />
                    {t("mypc.disk.selectAll")}
                  </label>
                  <Segmented<"size" | "name">
                    size="sm"
                    aria-label={t("mypc.disk.sortLabel")}
                    value={sortBy}
                    onChange={setSortBy}
                    options={[
                      { value: "size", label: t("mypc.disk.sortSize") },
                      { value: "name", label: t("mypc.disk.sortName") },
                    ]}
                  />
                </div>

                <ul className="flex flex-col">
                  {sortedChildren.map((item) => (
                    <DiskRow
                      key={item.path}
                      item={item}
                      t={t}
                      selected={selectedPaths.has(item.path)}
                      calculating={pendingSizePaths.has(item.path)}
                      deleting={deletingPaths.has(item.path)}
                      pending={pendingDeletePath === item.path}
                      confirming={confirmingDeletePath === item.path}
                      bulkBusy={bulkDeleteBusy}
                      onToggle={() => toggleSelected(item)}
                      onOpen={() => void openPath(item.path)}
                      onAskDelete={() => setConfirmingDeletePath(item.path)}
                      onCancelDelete={() => setConfirmingDeletePath(null)}
                      onConfirmDelete={() => void deleteItem(item)}
                    />
                  ))}
                </ul>
              </div>
            </div>
          )}

          {/* Barra fixa de seleção: vale para itens marcados em qualquer pasta. */}
          {(selectedItems.length > 0 || bulkProgress) && (
            <div className="sticky bottom-0 z-10 flex flex-wrap items-center gap-x-5 gap-y-2 rounded-control border border-line bg-surface-2 px-4 py-3">
              {bulkProgress ? (
                <div className="flex min-w-[220px] flex-1 flex-col gap-1.5">
                  <span className="text-sm font-semibold tabular-nums text-fg">
                    {t("mypc.disk.bulkDeleting", { done: bulkProgress.done + 1, total: bulkProgress.total })}
                  </span>
                  <Progress
                    value={(bulkProgress.done / bulkProgress.total) * 100}
                    label={t("mypc.disk.bulkDeleting", { done: bulkProgress.done + 1, total: bulkProgress.total })}
                  />
                </div>
              ) : (
                <>
                  <span className="text-sm font-semibold tabular-nums text-fg">
                    {selectedItems.length === 1
                      ? t("mypc.disk.selectedSummaryOne", { size: formatBytes(selectedTotalBytes) })
                      : t("mypc.disk.selectedSummary", { count: selectedItems.length, size: formatBytes(selectedTotalBytes) })}
                  </span>
                  <p className="min-w-[220px] flex-1 text-xs leading-relaxed text-fg-3">
                    {t("mypc.disk.selectionRule", { limit: limitText })}
                  </p>
                  <div className="flex items-center gap-4">
                    <Button variant="link" className="text-[13px]" disabled={bulkDeleteBusy} onClick={() => setSelectedPaths(new Map())}>
                      {t("mypc.disk.clearSelection")}
                    </Button>
                    <Button
                      size="sm"
                      variant={selectionHasPermanent ? "danger" : "outline"}
                      disabled={bulkDeleteBusy}
                      onClick={() => setConfirmingBulkDelete(true)}
                    >
                      {t("mypc.disk.deleteSelected")}
                    </Button>
                  </div>
                </>
              )}
            </div>
          )}
        </Card>
      )}

      {confirmingBulkDelete && (
        <BulkDeleteConfirmDialog
          items={selectedItems}
          limitText={limitText}
          onRemoveItem={removeFromSelection}
          onConfirm={() => void deleteSelected()}
          onCancel={() => setConfirmingBulkDelete(false)}
          t={t}
        />
      )}

      {actionError && <ErrorDialog message={actionError} onClose={() => setActionError(null)} t={t} />}
    </div>
  );
}

function DiskRow({
  item,
  t,
  selected,
  calculating,
  deleting,
  pending,
  confirming,
  bulkBusy,
  onToggle,
  onOpen,
  onAskDelete,
  onCancelDelete,
  onConfirmDelete,
}: {
  item: DiskTreeNodeSummary;
  t: Translate;
  selected: boolean;
  calculating: boolean;
  deleting: boolean;
  pending: boolean;
  confirming: boolean;
  bulkBusy: boolean;
  onToggle: () => void;
  onOpen: () => void;
  onAskDelete: () => void;
  onCancelDelete: () => void;
  onConfirmDelete: () => void;
}) {
  const permanent = isPermanent(item);
  return (
    <li
      className={cn(
        "flex flex-col border-b border-line-soft transition-all duration-200 ease-in last:border-b-0",
        deleting ? "-translate-x-2 opacity-0" : pending ? "opacity-60" : "opacity-100",
      )}
      style={deleting ? { maxHeight: 0, overflow: "hidden" } : undefined}
    >
      <div className={cn("flex min-h-12 items-center gap-3 rounded-control px-2", selected && "bg-accent/5")}>
        {item.actionable ? (
          <input
            type="checkbox"
            checked={selected}
            onChange={onToggle}
            disabled={bulkBusy || pending || deleting}
            aria-label={t("mypc.disk.selectItem", { name: item.name })}
            className="size-4 shrink-0 accent-accent disabled:cursor-not-allowed"
          />
        ) : (
          <span className="size-4 shrink-0" aria-hidden />
        )}
        {item.isDir ? (
          <Folder className="size-4 shrink-0 text-accent" aria-hidden />
        ) : (
          <File className="size-4 shrink-0 text-violet" aria-hidden />
        )}
        {item.isDir ? (
          <button
            type="button"
            onClick={onOpen}
            title={item.path}
            aria-label={t("mypc.disk.openFolder", { name: item.name })}
            className="min-w-0 flex-1 truncate rounded-sm text-left text-sm text-fg transition-colors duration-fast hover:text-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
          >
            {item.name}
          </button>
        ) : (
          <span className="min-w-0 flex-1 truncate text-sm text-fg-2" title={item.path}>
            {item.name}
          </span>
        )}
        {!item.actionable &&
          (item.protected ? (
            <StatusPill tone="warn" icon={<Shield aria-hidden />} className="shrink-0">
              <span title={t("mypc.disk.protectedHint")}>{t("mypc.disk.protectedPill")}</span>
            </StatusPill>
          ) : (
            <StatusPill tone="warn" icon={<Lock aria-hidden />} className="shrink-0">
              <span title={t("mypc.disk.systemHint")}>{t("mypc.disk.systemPill")}</span>
            </StatusPill>
          ))}
        <span className="flex w-20 shrink-0 items-center justify-end text-right text-[13px] tabular-nums text-fg-2">
          {calculating ? (
            <span className="flex items-center gap-1 text-fg-4">
              <RefreshCw className="size-3 animate-spin" aria-hidden />
              <span className="sr-only">{t("mypc.disk.calculatingSize")}</span>
            </span>
          ) : (
            formatBytes(item.sizeBytes)
          )}
        </span>
        {item.actionable ? (
          pending ? (
            <span className="flex h-9 shrink-0 items-center gap-1.5 text-xs text-danger">
              <RefreshCw className="size-3.5 animate-spin" aria-hidden />
              {t("mypc.disk.deleting")}
            </span>
          ) : (
            <button
              type="button"
              disabled={bulkBusy || confirming}
              onClick={onAskDelete}
              title={t("mypc.disk.deleteHint", { name: item.name })}
              aria-label={t("mypc.disk.deleteHint", { name: item.name })}
              className="grid size-9 shrink-0 place-items-center rounded-control border border-line text-fg-3 transition-colors duration-fast hover:border-danger-line hover:text-danger focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 disabled:cursor-not-allowed disabled:opacity-40"
            >
              <Trash2 className="size-4" aria-hidden />
            </button>
          )
        ) : (
          <span className="size-9 shrink-0" aria-hidden />
        )}
      </div>
      {confirming && !pending && (
        <ConfirmInline
          className="mb-2 mt-1"
          danger={permanent}
          message={
            permanent ? (
              <>
                <strong className="font-semibold">
                  {t("mypc.disk.confirmPermanentLead", { size: formatBytes(item.sizeBytes) })}
                </strong>{" "}
                {t("mypc.disk.confirmPermanentRest")}
                <span className="mt-0.5 block truncate text-xs text-fg-3" title={item.path}>
                  {t("mypc.disk.confirmPermanentPath", { path: item.path })}
                </span>
              </>
            ) : (
              t("mypc.disk.confirmQuarantine")
            )
          }
          confirmLabel={permanent ? t("mypc.disk.confirmPermanentAction") : t("mypc.disk.confirmQuarantineAction")}
          onCancel={onCancelDelete}
          onConfirm={onConfirmDelete}
        />
      )}
    </li>
  );
}

/** Aparece logo antes do apagar em lote - único lugar em que o usuário vê
 * tudo o que marcou, inclusive em pastas que já deixou. Dá para tirar itens
 * daqui sem voltar à pasta de origem. */
function BulkDeleteConfirmDialog({
  items,
  limitText,
  onRemoveItem,
  onConfirm,
  onCancel,
  t,
}: {
  items: DiskTreeNodeSummary[];
  limitText: string;
  onRemoveItem: (path: string) => void;
  onConfirm: () => void;
  onCancel: () => void;
  t: Translate;
}) {
  // Tirou o último item: não sobra nada para confirmar, fecha.
  useEffect(() => {
    if (items.length === 0) onCancel();
  }, [items.length, onCancel]);

  if (items.length === 0) return null;

  const folderCount = items.filter((item) => item.isDir).length;
  const fileCount = items.length - folderCount;
  const totalBytes = items.reduce((sum, item) => sum + item.sizeBytes, 0);
  const permanentCount = items.filter(isPermanent).length;

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-bg/80 px-4">
      <section
        role="alertdialog"
        aria-modal="true"
        aria-label={t("mypc.disk.bulkConfirmTitle", { count: items.length })}
        className="flex w-full max-w-lg flex-col rounded-card border border-line bg-surface p-6"
      >
        <h2 className="text-base font-semibold text-fg">{t("mypc.disk.bulkConfirmTitle", { count: items.length })}</h2>
        <p className="mt-2 text-sm leading-relaxed text-fg-2">
          {t("mypc.disk.bulkConfirmSummary", { folders: folderCount, files: fileCount, size: formatBytes(totalBytes) })}
        </p>
        {permanentCount > 0 && (
          <div className="mt-3 flex items-start gap-2 rounded-control border border-danger-line bg-danger/5 px-3 py-2 text-[13px] text-danger">
            <AlertTriangle className="mt-0.5 size-4 shrink-0" aria-hidden />
            {t("mypc.disk.bulkPermanentWarning", { count: permanentCount, limit: limitText })}
          </div>
        )}

        <ul className="mt-4 max-h-64 overflow-y-auto rounded-control border border-line bg-bg/40">
          {items.map((item) => {
            const permanent = isPermanent(item);
            const parent = parentPath(item.path);
            return (
              <li key={item.path} className="flex items-center gap-2.5 border-b border-line-soft px-3 py-2 text-sm last:border-b-0">
                {item.isDir ? (
                  <Folder className="size-4 shrink-0 text-accent" aria-hidden />
                ) : (
                  <File className="size-4 shrink-0 text-violet" aria-hidden />
                )}
                <div className="min-w-0 flex-1">
                  <div className="truncate text-fg" title={item.path}>
                    {item.name}
                  </div>
                  {parent && <div className="truncate text-[11px] text-fg-4">{parent}</div>}
                </div>
                {permanent && (
                  <span className="shrink-0 text-[11px] text-danger">{t("mypc.disk.bulkPermanentItem")}</span>
                )}
                <span className="w-16 shrink-0 text-right text-xs tabular-nums text-fg-2">{formatBytes(item.sizeBytes)}</span>
                <button
                  type="button"
                  onClick={() => onRemoveItem(item.path)}
                  title={t("mypc.disk.removeFromSelection")}
                  aria-label={t("mypc.disk.removeFromSelection")}
                  className="grid size-8 shrink-0 place-items-center rounded-control text-fg-3 transition-colors duration-fast hover:text-fg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
                >
                  <X className="size-4" aria-hidden />
                </button>
              </li>
            );
          })}
        </ul>

        <div className="mt-6 flex justify-end gap-2">
          <Button variant="outline" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
          <Button variant={permanentCount > 0 ? "danger" : "outline"} onClick={onConfirm} autoFocus>
            {t("mypc.disk.deleteSelected")}
          </Button>
        </div>
      </section>
    </div>
  );
}

function ErrorDialog({ message, onClose, t }: { message: string; onClose: () => void; t: Translate }) {
  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-bg/80 px-4">
      <section
        role="alertdialog"
        aria-modal="true"
        aria-label={t("mypc.disk.errorDialogTitle")}
        className="w-full max-w-md rounded-card border border-danger-line bg-surface p-6"
      >
        <h2 className="flex items-center gap-2 text-base font-semibold text-danger">
          <AlertTriangle className="size-4" aria-hidden />
          {t("mypc.disk.errorDialogTitle")}
        </h2>
        <p className="mt-3 whitespace-pre-line text-sm leading-relaxed text-fg-2">{message}</p>
        <div className="mt-6 flex justify-end">
          <Button variant="outline" onClick={onClose} autoFocus>
            {t("common.close")}
          </Button>
        </div>
      </section>
    </div>
  );
}
