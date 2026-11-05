import { runPerformanceScan, type PerformanceReport } from "@/services/tauri/agent";
import { createAgentResource } from "@/features/shared/agentResource";

export type ScanMode = "baseline" | "after" | "quick";

/**
 * Último relatório da Medição de desempenho (antes/depois). Não é a Saúde do
 * Dashboard: são métricas diferentes e nunca aparecem sem rótulo (doc 14 Q3).
 */
export const performanceReport = createAgentResource<PerformanceReport, [ScanMode?]>({
  name: "performance.report",
  fetch: (mode = "quick") => runPerformanceScan(mode),
});

export function usePerformanceScan(options?: { auto?: boolean }) {
  const resource = performanceReport.use(options);
  return {
    report: resource.data ?? null,
    loading: resource.loading,
    error: resource.error,
    refresh: resource.refresh,
    set: resource.set,
  };
}
