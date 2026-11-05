import { scanStartupImpact, type StartupImpact } from "@/services/tauri/agent";
import { createAgentResource } from "@/features/shared/agentResource";

/** Apps que abrem com o Windows e o impacto de cada um (alto/médio/baixo). Fonte única (doc 14 Q1). */
export const startupImpact = createAgentResource<StartupImpact[]>({
  name: "startup.impact",
  fetch: scanStartupImpact,
});

export function useStartup(options?: { auto?: boolean }) {
  const resource = startupImpact.use(options);
  return {
    apps: resource.data ?? [],
    loading: resource.loading,
    error: resource.error,
    refresh: resource.refresh,
    set: resource.set,
  };
}
