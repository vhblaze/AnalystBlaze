import { getEnergyDiagnostics, type EnergyDiagnostics } from "@/services/tauri/agent";
import { createAgentResource } from "@/features/shared/agentResource";

/** Plano de energia, tomada/bateria e diagnóstico de energia do Windows. */
export const energyDiagnostics = createAgentResource<EnergyDiagnostics>({
  name: "health.energy",
  fetch: getEnergyDiagnostics,
});

export function useEnergyDiagnostics(options?: { auto?: boolean }) {
  const resource = energyDiagnostics.use(options);
  return {
    energy: resource.data ?? null,
    loading: resource.loading,
    error: resource.error,
    refresh: resource.refresh,
    set: resource.set,
  };
}
