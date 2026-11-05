import { getUsageProfile, type UsageProfile } from "@/services/tauri/agent";
import { createAgentResource } from "@/features/shared/agentResource";

/** Perfil de uso medido deste PC. Fonte única: Dashboard e, depois, Insights e Configurações leem daqui. */
export const usageProfile = createAgentResource<UsageProfile>({
  name: "profile.usage",
  fetch: getUsageProfile,
  staleMs: 5 * 60_000,
});

/** "19h, 21h e 23h" a partir de [19, 21, 23]. */
export function formatHours(hours: number[], and: string): string {
  const parts = hours.map((hour) => `${hour}h`);
  if (parts.length <= 1) return parts.join("");
  return `${parts.slice(0, -1).join(", ")} ${and} ${parts[parts.length - 1]}`;
}

/** Categorias ordenadas por parcela, sem "system" e sem zeros. */
export function sortedShares(shares: Record<string, number>): [string, number][] {
  return Object.entries(shares)
    .filter(([key, value]) => key !== "system" && value > 0)
    .sort((a, b) => b[1] - a[1]);
}
