import {
  getAuditLog,
  getOptimizationSnapshots,
  getTodaysAutomaticActions,
  type AuditEvent,
  type AutomaticActionEntry,
  type OptimizationSnapshot,
} from "@/services/tauri/agent";
import { createAgentResource } from "@/features/shared/agentResource";

const HISTORY_LIMIT = 120;

export const auditLog = createAgentResource<AuditEvent[]>({
  name: "history.audit",
  fetch: () => getAuditLog(HISTORY_LIMIT),
});

export const optimizationSnapshots = createAgentResource<OptimizationSnapshot[]>({
  name: "history.snapshots",
  fetch: () => getOptimizationSnapshots(HISTORY_LIMIT),
});

export const todaysAutomaticActions = createAgentResource<AutomaticActionEntry[]>({
  name: "history.todayAutomatic",
  fetch: getTodaysAutomaticActions,
  staleMs: 60_000,
});

/** Auditoria + snapshots: a mesma linha do tempo em Controles e no Histórico. */
export function useHistory(options?: { auto?: boolean }) {
  const audit = auditLog.use(options);
  const snapshots = optimizationSnapshots.use(options);
  return {
    audit: audit.data ?? [],
    snapshots: snapshots.data ?? [],
    loading: audit.loading || snapshots.loading,
    error: audit.error ?? snapshots.error,
    refresh: () => Promise.all([audit.refresh(), snapshots.refresh()]),
  };
}

/** O que o AnalystBlaze fez sozinho hoje (Dashboard e resumo do dia). */
export function useTodaysAutomaticActions(options?: { auto?: boolean }) {
  const resource = todaysAutomaticActions.use(options);
  return { actions: resource.data ?? [], loading: resource.loading, error: resource.error, refresh: resource.refresh };
}
