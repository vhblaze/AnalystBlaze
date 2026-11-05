import { listAllNetworkAdapters, listNetworkAdapters, type NetworkAdapterSummary } from "@/services/tauri/agent";
import { createAgentResource } from "@/features/shared/agentResource";

/** Adaptadores em uso (com IP). */
export const networkAdapters = createAgentResource<NetworkAdapterSummary[]>({
  name: "network.adapters",
  fetch: listNetworkAdapters,
});

/** Todos os adaptadores, inclusive desativados: a lista de "ligar/desligar". */
export const allNetworkAdapters = createAgentResource<NetworkAdapterSummary[]>({
  name: "network.allAdapters",
  fetch: listAllNetworkAdapters,
});
