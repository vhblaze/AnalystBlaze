import type { AgentTelemetrySnapshot } from "@/services/tauri/agent";

/**
 * Meu PC › Saúde do Windows (board Tela-MeuPC-Saude): o que está errado no
 * Windows e não é desempenho. Tudo vem do que o agente já mede em
 * `telemetry.advanced`; nada é estimado. Estes itens saíram do Insights
 * (doc 14 §1.3: diagnóstico do Windows mora em Meu PC).
 */

/** Mesmo corte que o Insights usava para "muitos erros críticos". */
export const EVENT_LOG_CRITICAL_THRESHOLD = 10;

/** Códigos do Gerenciador de Dispositivos (ConfigManagerErrorCode). */
export const PROBLEM_CODE_KEYS: Record<number, string> = {
  10: "cannotStart",
  12: "noResources",
  14: "needsRestart",
  18: "reinstallDrivers",
  19: "registryProblem",
  28: "driversNotInstalled",
  31: "couldNotStart",
  37: "driverError",
  38: "previousInstance",
  39: "driverMissing",
  40: "registryEntryCorrupt",
  43: "stoppedByWindows",
  48: "blocked",
};

type Advanced = {
  windows_update_reboot_pending?: boolean | null;
  event_log_critical_errors_24h?: number | null;
  failing_services?: Array<{ name: string; kind: string }>;
  shell_crashes?: Array<{ process: string }>;
  failing_devices?: Array<{ name?: string | null; device_id: string; device_class?: string | null; problem_code: number }>;
  modem_usb_link_issue?: { usb_device_id?: string | null; root_net_device_id?: string | null } | null;
};

export type HealthAction =
  | { kind: "restart_windows" }
  | { kind: "system_file_check" }
  | { kind: "restart_device"; deviceId: string }
  | { kind: "shadow_consent" };

export type HealthItem = {
  id: string;
  severity: "danger" | "warn" | "ok";
  /** Chave i18n em mypc.health.items.* com parâmetros. */
  titleKey: string;
  titleParams?: Record<string, string | number>;
  detailKey: string;
  detailParams?: Record<string, string | number>;
  action?: HealthAction;
};

export function windowsHealthItems({
  telemetry,
  shadowConsentNeeded,
}: {
  telemetry: AgentTelemetrySnapshot | null;
  shadowConsentNeeded: boolean;
}): HealthItem[] {
  const advanced = (telemetry?.advanced ?? null) as Advanced | null;
  const items: HealthItem[] = [];

  if (advanced?.windows_update_reboot_pending === true) {
    items.push({ id: "reboot", severity: "danger", titleKey: "reboot.title", detailKey: "reboot.detail", action: { kind: "restart_windows" } });
  }

  if (shadowConsentNeeded) {
    items.push({ id: "shadow", severity: "warn", titleKey: "shadow.title", detailKey: "shadow.detail", action: { kind: "shadow_consent" } });
  }

  const critical = advanced?.event_log_critical_errors_24h ?? 0;
  const services = advanced?.failing_services ?? [];
  const crashes = advanced?.shell_crashes ?? [];
  if (critical >= EVENT_LOG_CRITICAL_THRESHOLD || services.length > 0) {
    const parts: string[] = [];
    if (services.length > 0) parts.push(services.slice(0, 3).map((service) => service.name).join(", "));
    items.push({
      id: "system",
      severity: "warn",
      titleKey: services.length > 0 ? "services.title" : "events.title",
      titleParams: { count: services.length > 0 ? services.length : critical },
      detailKey: crashes.length > 0 ? "services.detailWithCrashes" : "services.detail",
      detailParams: { names: parts.join(" · ") || "-", events: critical, crashes: crashes.length },
      action: { kind: "system_file_check" },
    });
  }

  // O modem USB instável tem card próprio no Insights; os dois dispositivos
  // que ele explica não viram itens genéricos aqui.
  const correlated = new Set(
    [advanced?.modem_usb_link_issue?.usb_device_id, advanced?.modem_usb_link_issue?.root_net_device_id].filter(
      (id): id is string => Boolean(id),
    ),
  );
  for (const device of advanced?.failing_devices ?? []) {
    if (correlated.has(device.device_id)) continue;
    items.push({
      id: `device:${device.device_id}`,
      severity: "warn",
      titleKey: "device.title",
      titleParams: { name: device.name?.trim() || device.device_class || "?" },
      detailKey: `codes.${PROBLEM_CODE_KEYS[device.problem_code] ?? "other"}`,
      detailParams: { code: device.problem_code },
      action: { kind: "restart_device", deviceId: device.device_id },
    });
  }

  if (advanced) {
    if (advanced.windows_update_reboot_pending === false) {
      items.push({ id: "reboot-ok", severity: "ok", titleKey: "reboot.okTitle", detailKey: "reboot.okDetail" });
    }
    if (critical < EVENT_LOG_CRITICAL_THRESHOLD && services.length === 0) {
      items.push({ id: "system-ok", severity: "ok", titleKey: "services.okTitle", detailKey: "services.okDetail", detailParams: { events: critical } });
    }
    if ((advanced.failing_devices ?? []).length === 0) {
      items.push({ id: "devices-ok", severity: "ok", titleKey: "device.okTitle", detailKey: "device.okDetail" });
    }
  }

  return items;
}
