import { getPrivilegedHelperStatus, type PrivilegedHelperStatus } from "@/services/tauri/agent";
import { createAgentResource } from "@/features/shared/agentResource";

/** Estado do componente de sistema (helper privilegiado). Uma leitura para o app inteiro. */
export const privilegedHelperStatus = createAgentResource<PrivilegedHelperStatus>({
  name: "helper.status",
  fetch: getPrivilegedHelperStatus,
  staleMs: 15_000,
});

/** O componente está pronto para ações que pedem administrador? */
export function helperReady(status?: PrivilegedHelperStatus | null): boolean {
  return Boolean(status?.available && !status.requiresUpdate);
}

export function useHelper(options?: { auto?: boolean }) {
  const resource = privilegedHelperStatus.use(options);
  return {
    status: resource.data ?? null,
    ready: helperReady(resource.data),
    loading: resource.loading,
    error: resource.error,
    refresh: resource.refresh,
    set: resource.set,
  };
}
