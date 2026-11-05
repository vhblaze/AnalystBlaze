import {
  getLocalAiPolicy,
  getWeeklyAiUsage,
  listenToWeeklyAiUsage,
  saveLocalAiPolicy,
  type LocalAiPolicy,
  type WeeklyAiUsage,
} from "@/services/tauri/agent";
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

/** Minutos de automação usados nesta semana (Starter). Atualiza por evento do agente. */
export const weeklyAutomationUsage = createAgentResource<WeeklyAiUsage | null>({
  name: "automations.weeklyUsage",
  fetch: getWeeklyAiUsage,
  subscribe: (push) => listenToWeeklyAiUsage(push),
});

export function useWeeklyAutomationUsage() {
  return weeklyAutomationUsage.use().data ?? null;
}
