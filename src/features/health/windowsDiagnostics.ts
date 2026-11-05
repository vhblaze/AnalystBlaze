import type { AgentTelemetrySnapshot, EnergyDiagnostics } from "@/services/tauri/agent";

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

/** Driver da GPU com mais que isto é "antigo" (mesmo corte do agente, OUTDATED_DRIVER_AGE_DAYS). */
export const OLD_DRIVER_DAYS = 180;

type Advanced = {
  defender_status?: string | null;
  defender_realtime_enabled?: boolean | null;
  gpu_driver_status?: { device_name?: string | null; driver_version?: string | null; driver_age_days?: number | null; outdated?: boolean | null } | null;
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
  | { kind: "shadow_consent" }
  | { kind: "repair_service"; serviceName: string }
  | { kind: "open_security" }
  | { kind: "open_energy" };

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
  energy = null,
  thermalLimits = null,
}: {
  telemetry: AgentTelemetrySnapshot | null;
  shadowConsentNeeded: boolean;
  /** Plano de energia ativo e o recomendado (features/energy). */
  energy?: EnergyDiagnostics | null;
  /** Limites de temperatura da política local (Controles › Automações). */
  thermalLimits?: { cpu: number; gpu: number } | null;
}): HealthItem[] {
  const advanced = (telemetry?.advanced ?? null) as Advanced | null;
  const items: HealthItem[] = [];

  // Defender: só o que o agente lê de Get-MpComputerStatus.
  if (advanced?.defender_realtime_enabled === false) {
    items.push({ id: "defender", severity: "danger", titleKey: "defender.offTitle", detailKey: "defender.offDetail", action: { kind: "open_security" } });
  } else if (advanced?.defender_status === "attention") {
    items.push({ id: "defender", severity: "warn", titleKey: "defender.attentionTitle", detailKey: "defender.attentionDetail", action: { kind: "open_security" } });
  }

  // Temperatura: só quando há sensor; sem sensor, nenhum item (nunca "--").
  const cpuTemp = telemetry?.cpu_temperature_available !== false && typeof telemetry?.cpu_temperature === "number" && telemetry.cpu_temperature > 0 ? Math.round(telemetry.cpu_temperature) : null;
  const gpuTemp = telemetry?.gpu_temperature_available !== false && typeof telemetry?.gpu_temperature === "number" && telemetry.gpu_temperature > 0 ? Math.round(telemetry.gpu_temperature) : null;
  const cpuHot = cpuTemp !== null && thermalLimits !== null && cpuTemp >= thermalLimits.cpu;
  const gpuHot = gpuTemp !== null && thermalLimits !== null && gpuTemp >= thermalLimits.gpu;
  if (cpuHot || gpuHot) {
    items.push({
      id: "temperature",
      severity: "warn",
      titleKey: cpuHot && gpuHot ? "temperature.hotBothTitle" : cpuHot ? "temperature.hotCpuTitle" : "temperature.hotGpuTitle",
      titleParams: { cpu: cpuTemp ?? 0, gpu: gpuTemp ?? 0 },
      detailKey: "temperature.hotDetail",
      detailParams: { cpuLimit: thermalLimits?.cpu ?? 0, gpuLimit: thermalLimits?.gpu ?? 0 },
    });
  }

  // Energia: na tomada com o plano de economia (ou o contrário) é aviso, com link.
  const activePlan = energy?.active_scheme_alias ?? null;
  const recommended = energy?.recommended_plan ?? null;
  if (activePlan && recommended && activePlan !== recommended) {
    items.push({
      id: "energy",
      severity: "warn",
      titleKey: "energy.mismatchTitle",
      titleParams: { source: energy?.power_source === "battery" ? "battery" : "ac" },
      detailKey: "energy.mismatchDetail",
      action: { kind: "open_energy" },
    });
  }

  // Serviços falhando: um item por serviço, com "tentar corrigir" (REPAIR_SERVICE).
  for (const service of (advanced?.failing_services ?? []).slice(0, 5)) {
    items.push({
      id: `service:${service.name}`,
      severity: "warn",
      titleKey: "service.title",
      titleParams: { name: service.name },
      detailKey: "service.detail",
      action: { kind: "repair_service", serviceName: service.name },
    });
  }

  if (advanced?.windows_update_reboot_pending === true) {
    items.push({ id: "reboot", severity: "danger", titleKey: "reboot.title", detailKey: "reboot.detail", action: { kind: "restart_windows" } });
  }

  if (shadowConsentNeeded) {
    items.push({ id: "shadow", severity: "warn", titleKey: "shadow.title", detailKey: "shadow.detail", action: { kind: "shadow_consent" } });
  }

  const critical = advanced?.event_log_critical_errors_24h ?? 0;
  const services = advanced?.failing_services ?? [];
  const crashes = advanced?.shell_crashes ?? [];
  if (critical >= EVENT_LOG_CRITICAL_THRESHOLD) {
    items.push({
      id: "system",
      severity: "warn",
      titleKey: "events.title",
      titleParams: { count: critical },
      detailKey: crashes.length > 0 ? "events.detailWithCrashes" : "events.detail",
      detailParams: { events: critical, crashes: crashes.length },
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
    if (advanced.defender_status === "healthy") {
      items.push({ id: "defender-ok", severity: "ok", titleKey: "defender.okTitle", detailKey: "defender.okDetail" });
    }
    const driver = advanced.gpu_driver_status;
    if (driver?.driver_version) {
      const name = driver.device_name?.trim() || "GPU";
      const age = typeof driver.driver_age_days === "number" ? driver.driver_age_days : null;
      const old = driver.outdated === true || (age !== null && age >= OLD_DRIVER_DAYS);
      items.push({
        id: "driver",
        severity: old ? "warn" : "ok",
        titleKey: old ? "driver.oldTitle" : "driver.okTitle",
        titleParams: { name, version: driver.driver_version },
        detailKey: old ? "driver.oldDetail" : age !== null ? "driver.okDetail" : "driver.okDetailNoAge",
        detailParams: { days: age ?? 0 },
      });
    }
    if (!cpuHot && !gpuHot && (cpuTemp !== null || gpuTemp !== null)) {
      items.push({
        id: "temperature-ok",
        severity: "ok",
        titleKey: "temperature.okTitle",
        detailKey: cpuTemp !== null && gpuTemp !== null ? "temperature.okBoth" : cpuTemp !== null ? "temperature.okCpu" : "temperature.okGpu",
        detailParams: { cpu: cpuTemp ?? 0, gpu: gpuTemp ?? 0 },
      });
    }
    if (activePlan && (!recommended || activePlan === recommended)) {
      items.push({ id: "energy-ok", severity: "ok", titleKey: "energy.okTitle", detailKey: "energy.okDetail", detailParams: { plan: energy?.active_scheme_name ?? activePlan } });
    }
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
