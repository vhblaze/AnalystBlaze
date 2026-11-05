import { getNetworkDiagnostics, type NetworkDiagnostics } from "@/services/tauri/agent";
import { createAgentResource } from "@/features/shared/agentResource";

/** Diagnóstico de rede (ping, roteador, perda, adaptador). A mesma leitura em Insights e em Meu PC › Rede. */
export const networkDiagnostics = createAgentResource<NetworkDiagnostics>({
  name: "network.diagnostics",
  fetch: getNetworkDiagnostics,
  staleMs: 20_000,
});

export function useNetwork(options?: { auto?: boolean }) {
  const resource = networkDiagnostics.use(options);
  return {
    diagnostics: resource.data ?? null,
    loading: resource.loading,
    error: resource.error,
    refresh: resource.refresh,
    set: resource.set,
  };
}
