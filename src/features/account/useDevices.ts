import { listAccountDevices, removeAccountDevice, type AccountDevice } from "@/services/tauri/agent";
import { createAgentResource } from "@/features/shared/agentResource";

/** PCs da conta, como o servidor lista. O PC atual é o que tem o hw_id do status. */
export const accountDevices = createAgentResource<AccountDevice[]>({
  name: "account.devices",
  fetch: listAccountDevices,
  staleMs: 60_000,
});

/** Tira um PC da conta e relê a lista. Se for este PC, a sessão termina. */
export async function removeDevice(hwId: string) {
  const status = await removeAccountDevice(hwId);
  accountDevices.refresh().catch(() => undefined);
  return status;
}

export function useAccountDevices(options?: { auto?: boolean }) {
  const resource = accountDevices.use(options);
  return { devices: resource.data ?? [], loading: resource.loading, error: resource.error, refresh: resource.refresh };
}
