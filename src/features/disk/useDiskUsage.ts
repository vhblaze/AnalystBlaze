import {
  checkDiskOptimizationInsight,
  getDiskDirectDeleteThresholdBytes,
  getDiskUsageSummary,
  listDiskDirectory,
  type DiskOptimizationInsight,
  type DiskTreeNodeSummary,
  type DiskUsageSummary,
} from "@/services/tauri/agent";
import { createAgentResource } from "@/features/shared/agentResource";

/** Resumo de uso do disco por categoria: a mesma leitura em Disco e em Insights. */
export const diskUsageSummary = createAgentResource<DiskUsageSummary>({
  name: "disk.usageSummary",
  fetch: () => getDiskUsageSummary(),
  staleMs: 5 * 60_000,
});

/** HD com a otimização agendada desligada: lido uma vez por sessão, usado em Disco e em Insights. */
export const diskOptimizationInsight = createAgentResource<DiskOptimizationInsight | null>({
  name: "disk.optimizationInsight",
  fetch: checkDiskOptimizationInsight,
  staleMs: 10 * 60_000,
});

/**
 * Limite acima do qual o agente apaga de vez em vez de pôr em quarentena.
 * Vem do agente (doc 13 §6): a tela nunca repete o número.
 */
export const diskDirectDeleteThreshold = createAgentResource<number>({
  name: "disk.directDeleteThreshold",
  fetch: getDiskDirectDeleteThresholdBytes,
  staleMs: Number.POSITIVE_INFINITY,
});

const directoryCache = new Map<string, DiskTreeNodeSummary[]>();
const directoryInflight = new Map<string, Promise<DiskTreeNodeSummary[]>>();

/** Filhos imediatos de uma pasta, com cache por caminho e uma leitura por vez. */
export function listDirectory(path: string, { fresh = false } = {}): Promise<DiskTreeNodeSummary[]> {
  if (!fresh) {
    const cached = directoryCache.get(path);
    if (cached) return Promise.resolve(cached);
    const inflight = directoryInflight.get(path);
    if (inflight) return inflight;
  }
  const request = listDiskDirectory(path)
    .then((items) => {
      directoryCache.set(path, items);
      return items;
    })
    .finally(() => directoryInflight.delete(path));
  directoryInflight.set(path, request);
  return request;
}

/** Esquece as pastas lidas (depois de apagar algo, por exemplo). */
export function forgetDirectories() {
  directoryCache.clear();
}
