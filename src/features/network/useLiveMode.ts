import { getLiveModeStatus, probeGatewayIdentity, type LiveModeStatus } from "@/services/tauri/agent";
import { createAgentResource } from "@/features/shared/agentResource";

/** Estado do Modo ao vivo (ligado, últimas amostras, bitrate, incidente). */
export const liveModeStatus = createAgentResource<LiveModeStatus>({
  name: "network.liveMode",
  fetch: getLiveModeStatus,
  staleMs: 2_000,
});

const gatewayCache = new Map<string, string | null>();

/** Quem é o roteador (fabricante/modelo), uma consulta por endereço. */
export async function identifyGateway(gateway: string): Promise<string | null> {
  if (gatewayCache.has(gateway)) return gatewayCache.get(gateway) ?? null;
  const found = await probeGatewayIdentity(gateway);
  gatewayCache.set(gateway, found ?? null);
  return found ?? null;
}
