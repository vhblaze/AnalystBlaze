import {
  addProtectedApp,
  getProtectedApps,
  getWindowsInventory,
  removeProtectedApp,
  type ProtectedApp,
  type WindowsInventory,
} from "@/services/tauri/agent";
import { createAgentResource } from "@/features/shared/agentResource";

/** Serviços e apps de inicialização que o Windows informa ao agente. */
export const windowsInventory = createAgentResource<WindowsInventory>({
  name: "windows.inventory",
  fetch: getWindowsInventory,
});

/** Serviços que o agente permite parar e restaurar (classificados como seguros). */
export function safeServices(inventory: WindowsInventory | null | undefined) {
  return (inventory?.services ?? []).filter((service) => service.can_modify && service.classification === "safe");
}

export function useWindowsInventory(options?: { auto?: boolean }) {
  const resource = windowsInventory.use(options);
  return {
    inventory: resource.data ?? null,
    services: safeServices(resource.data),
    loading: resource.loading,
    error: resource.error,
    refresh: resource.refresh,
  };
}

/** Apps em que o AnalystBlaze nunca mexe (prioridade, limpeza, Modo Jogo). */
export const protectedApps = createAgentResource<ProtectedApp[]>({
  name: "windows.protectedApps",
  fetch: getProtectedApps,
});

export async function protectApp(name: string): Promise<ProtectedApp[]> {
  const next = await addProtectedApp(name);
  protectedApps.set(next);
  return next;
}

export async function unprotectApp(name: string): Promise<ProtectedApp[]> {
  const next = await removeProtectedApp(name);
  protectedApps.set(next);
  return next;
}

export function useProtectedApps(options?: { auto?: boolean }) {
  const resource = protectedApps.use(options);
  return { apps: resource.data ?? [], loading: resource.loading, error: resource.error, refresh: resource.refresh };
}
