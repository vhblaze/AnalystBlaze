import { getIdleWeightCandidates, type IdleWeightItem } from "@/services/tauri/agent";
import { createAgentResource } from "@/features/shared/agentResource";

/** "Pesa sem você usar": apps de inicialização e serviços com custo agora e sem uso há 7 dias. */
export const idleWeight = createAgentResource<IdleWeightItem[]>({
  name: "profile.idleWeight",
  fetch: getIdleWeightCandidates,
  staleMs: 5 * 60_000,
});

const DISMISS_STORAGE = "analystblaze.idleWeight.dismissed.v1";
const DISMISS_TTL_MS = 24 * 60 * 60 * 1000;

export function idleWeightKey(item: IdleWeightItem): string {
  return `${item.kind}:${item.name.toLowerCase()}`;
}

export function loadDismissed(): Record<string, number> {
  try {
    const raw = JSON.parse(localStorage.getItem(DISMISS_STORAGE) ?? "{}") as Record<string, number>;
    const now = Date.now();
    return Object.fromEntries(Object.entries(raw).filter(([, at]) => now - at < DISMISS_TTL_MS));
  } catch {
    return {};
  }
}

export function saveDismissed(value: Record<string, number>) {
  try {
    localStorage.setItem(DISMISS_STORAGE, JSON.stringify(value));
  } catch {
    // Best effort.
  }
}
