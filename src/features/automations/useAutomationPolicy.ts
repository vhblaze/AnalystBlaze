import { getLocalAiPolicy, saveLocalAiPolicy, type LocalAiPolicy } from "@/services/tauri/agent";
import { createAgentResource } from "@/features/shared/agentResource";

/** Quais automações o usuário aprovou. Lida em Controles e em Configurações; gravada num lugar só. */
export const automationPolicy = createAgentResource<LocalAiPolicy>({
  name: "automations.policy",
  fetch: getLocalAiPolicy,
});

/** Grava a política e atualiza todas as telas que a mostram. */
export async function saveAutomationPolicy(policy: LocalAiPolicy): Promise<LocalAiPolicy> {
  const saved = await saveLocalAiPolicy(policy);
  automationPolicy.set(saved);
  return saved;
}

export function useAutomationPolicy(options?: { auto?: boolean }) {
  const resource = automationPolicy.use(options);
  return {
    policy: resource.data ?? null,
    loading: resource.loading,
    error: resource.error,
    refresh: resource.refresh,
    set: resource.set,
  };
}
