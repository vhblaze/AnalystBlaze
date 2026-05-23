import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrent, onOpenUrl } from "@tauri-apps/plugin-deep-link";

export type AgentStatus = {
  authenticated: boolean;
  registered: boolean;
  hw_id?: string | null;
  user_name?: string | null;
  user_email?: string | null;
  plan: string;
  has_paid_plan: boolean;
  mode: string;
  api_base_url: string;
  web_login_url: string;
  account_settings_url: string;
};

export type AgentTelemetrySample = {
  event_timestamp: number;
  cpu_usage: number;
  cpu_temperature?: number;
  cpu_temperature_available?: boolean;
  cpu_temperature_source?: string | null;
  cpu_temperature_methods?: Array<{
    source: string;
    label?: string | null;
    value_c?: number | null;
    available: boolean;
  }>;
  gpu_usage: number;
  gpu_usage_available?: boolean;
  gpu_name?: string;
  vram_gb?: number;
  vram_used_gb?: number | null;
  vram_usage_percent?: number | null;
  ram_usage_mb: number;
  ram_total_mb?: number;
  ram_usage_percent?: number;
  gpu_temperature: number;
  gpu_temperature_available?: boolean;
  latency_ms: number;
  disk_used_gb?: number;
  disk_total_gb?: number;
  disk_usage_percent?: number;
  active_processes?: number;
  system_uptime_seconds?: number;
  active_window?: string | null;
  idle_seconds?: number;
  advanced?: Record<string, unknown> | null;
  network?: NetworkDiagnostics | Record<string, unknown> | null;
};

export type AgentTelemetrySnapshot = AgentTelemetrySample & {
  health_score: number;
  health_level: "excellent" | "good" | "watch" | "critical" | string;
  health_reasons: string[];
  optimization_status: string;
  active_profile: string;
  telemetry_mode: string;
  device_online: boolean;
};

export type GameModeResult = {
  success: boolean;
  message: string;
  details: unknown;
  status: AgentStatus;
};

export type RestoreReport = {
  restored_snapshots: number;
  failed_snapshots: number;
  restored_entries: number;
  failed_entries: number;
  skipped_conflicts: number;
  messages: string[];
};

export type OptimizationResult = {
  success: boolean;
  message: string;
  details: unknown;
};

export type OptimizationPreview = {
  action_name: string;
  risk: string;
  requires_local_confirmation: boolean;
  requires_snapshot: boolean;
  requires_privileged_helper: boolean;
  allowed_without_helper: boolean;
  message: string;
};

export type AuditEvent = {
  timestamp: number;
  level: string;
  event: string;
  message: string;
  details: unknown;
};

export type OptimizationSnapshot = {
  id: string;
  action_name: string;
  created_at: number;
  restored_at?: number | null;
  entries: unknown[];
  details: unknown;
};

export type ProtectedApp = {
  name: string;
  reason?: string | null;
  created_at: number;
};

export type PrivilegedHelperStatus = {
  available: boolean;
  installed: boolean;
  version?: string | null;
  can_request_uac: boolean;
  supported_actions: string[];
  message: string;
};

export type WindowsInventory = {
  startup_apps: Array<{
    name: string;
    command: string;
    location: string;
    risk: string;
  }>;
  services: Array<{
    name: string;
    display_name?: string | null;
    start_type?: number | null;
    classification: string;
    can_modify: boolean;
  }>;
};

export type NetworkProbe = {
  label: string;
  target: string;
  sent: number;
  received: number;
  packet_loss_percent: number;
  avg_ms?: number | null;
  min_ms?: number | null;
  max_ms?: number | null;
  jitter_ms?: number | null;
};

export type NetworkDiagnostics = {
  connected: boolean;
  adapter_name?: string | null;
  adapter_description?: string | null;
  adapter_status?: string | null;
  adapter_type?: string | null;
  link_speed?: string | null;
  gateway?: string | null;
  dns_servers: string[];
  wifi_ssid?: string | null;
  wifi_signal_percent?: number | null;
  wifi_radio_type?: string | null;
  wifi_channel?: string | null;
  gateway_latency_ms?: number | null;
  dns_latency_ms?: number | null;
  external_latency_ms?: number | null;
  jitter_ms?: number | null;
  packet_loss_percent?: number | null;
  probes: NetworkProbe[];
  recommendations: string[];
  refreshed_at: number;
};

export type EnergyDiagnostics = {
  active_scheme_guid?: string | null;
  active_scheme_name?: string | null;
  active_scheme_alias?: string | null;
  power_source?: string | null;
  battery_percent?: number | null;
  battery_status?: string | null;
  battery_saver_on?: boolean | null;
  cpu_current_clock_mhz?: number | null;
  cpu_max_clock_mhz?: number | null;
  recommended_plan: string;
  recommendations: string[];
  refreshed_at: number;
};

export type LocalAiPolicy = {
  enabled: boolean;
  auto_game_mode: boolean;
  auto_restore_game_mode: boolean;
  optimize_power_plan: boolean;
  safe_temp_cleanup: boolean;
  manage_startup_apps: boolean;
  manage_services: boolean;
  reduce_background_processes: boolean;
  allow_automatic_sensitive_actions: boolean;
  require_confirmation_for_sensitive: boolean;
  max_risk: "safe" | "sensitive" | string;
  game_min_confidence: number;
  game_cooldown_seconds: number;
  cleanup_min_idle_seconds: number;
  cleanup_disk_threshold_percent: number;
  thermal_cpu_limit_c: number;
  thermal_gpu_limit_c: number;
  battery_saver_threshold_percent: number;
  network_latency_threshold_ms: number;
};

type SingleInstancePayload = {
  args?: unknown[];
  cwd?: string;
};

const fallbackStatus: AgentStatus = {
  authenticated: false,
  registered: false,
  hw_id: null,
  user_name: null,
  user_email: null,
  plan: "starter",
  has_paid_plan: false,
  mode: "stopped",
  api_base_url: import.meta.env.VITE_ANALYSTBLAZE_API_URL ?? "http://127.0.0.1:8000",
  web_login_url: import.meta.env.VITE_ANALYSTBLAZE_WEB_LOGIN_URL ?? "http://localhost:3000/login",
  account_settings_url: import.meta.env.VITE_ANALYSTBLAZE_ACCOUNT_URL ?? "http://localhost:3000/configuration",
};

export function isTauriRuntime() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function requireTauriRuntime(feature: string) {
  if (!isTauriRuntime()) {
    throw new Error(`${feature} exige o aplicativo desktop Tauri em execucao.`);
  }
}

export async function getAgentStatus() {
  if (!isTauriRuntime()) return fallbackStatus;
  return invoke<AgentStatus>("agent_status");
}

export async function openAgentLogin() {
  if (!isTauriRuntime()) return fallbackStatus.web_login_url;
  return invoke<string>("open_login");
}

export async function openAgentAccountSettings() {
  if (!isTauriRuntime()) return fallbackStatus.account_settings_url;
  return invoke<string>("open_account_settings");
}

export async function completeAuthFromDeepLink(rawUrl: string) {
  if (!isTauriRuntime()) return fallbackStatus;
  return invoke<AgentStatus>("complete_auth_from_deep_link", { rawUrl });
}

export async function startAgent() {
  if (!isTauriRuntime()) return fallbackStatus;
  return invoke<AgentStatus>("start_agent");
}

export async function activateAgentGameMode() {
  if (!isTauriRuntime()) throw new Error("Tauri runtime unavailable for game mode.");
  return invoke<GameModeResult>("activate_game_mode");
}

export async function restorePendingOptimizations(): Promise<RestoreReport> {
  requireTauriRuntime("Restauracao de snapshots");
  return invoke<RestoreReport>("restore_pending_optimizations");
}

export async function getOptimizationSnapshots(limit = 80): Promise<OptimizationSnapshot[]> {
  requireTauriRuntime("Historico de snapshots");
  return invoke<OptimizationSnapshot[]>("optimization_snapshots", { limit });
}

export async function getAuditLog(limit = 120): Promise<AuditEvent[]> {
  requireTauriRuntime("Auditoria local");
  return invoke<AuditEvent[]>("audit_log", { limit });
}

export async function getOptimizationPreview(
  actionName: string,
  payload?: Record<string, unknown> | null,
): Promise<OptimizationPreview> {
  if (!isTauriRuntime()) {
    return {
      action_name: actionName,
      risk: "sensitive",
      requires_local_confirmation: true,
      requires_snapshot: true,
      requires_privileged_helper: false,
      allowed_without_helper: true,
      message: "Preview local indisponivel fora do Tauri.",
    };
  }
  return invoke<OptimizationPreview>("optimization_preview", { actionName, payload });
}

export async function getWindowsInventory(): Promise<WindowsInventory> {
  requireTauriRuntime("Inventario real do Windows");
  return invoke<WindowsInventory>("windows_inventory");
}

export async function getNetworkDiagnostics(): Promise<NetworkDiagnostics> {
  requireTauriRuntime("Diagnostico real de rede");
  return invoke<NetworkDiagnostics>("network_diagnostics");
}

export async function getEnergyDiagnostics(): Promise<EnergyDiagnostics> {
  requireTauriRuntime("Diagnostico real de energia");
  return invoke<EnergyDiagnostics>("energy_diagnostics");
}

export async function getProtectedApps(): Promise<ProtectedApp[]> {
  requireTauriRuntime("Apps protegidos");
  return invoke<ProtectedApp[]>("protected_apps");
}

export async function addProtectedApp(name: string, reason?: string | null): Promise<ProtectedApp[]> {
  requireTauriRuntime("Apps protegidos");
  return invoke<ProtectedApp[]>("add_protected_app", { name, reason });
}

export async function removeProtectedApp(name: string): Promise<ProtectedApp[]> {
  requireTauriRuntime("Apps protegidos");
  return invoke<ProtectedApp[]>("remove_protected_app", { name });
}

export async function getPrivilegedHelperStatus(): Promise<PrivilegedHelperStatus> {
  requireTauriRuntime("Status do helper privilegiado");
  return invoke<PrivilegedHelperStatus>("privileged_helper_status");
}

export async function getLocalAiPolicy(): Promise<LocalAiPolicy> {
  if (!isTauriRuntime()) {
    return {
      enabled: false,
      auto_game_mode: true,
      auto_restore_game_mode: true,
      optimize_power_plan: true,
      safe_temp_cleanup: true,
      manage_startup_apps: false,
      manage_services: false,
      reduce_background_processes: false,
      allow_automatic_sensitive_actions: false,
      require_confirmation_for_sensitive: true,
      max_risk: "safe",
      game_min_confidence: 0.74,
      game_cooldown_seconds: 900,
      cleanup_min_idle_seconds: 900,
      cleanup_disk_threshold_percent: 90,
      thermal_cpu_limit_c: 88,
      thermal_gpu_limit_c: 84,
      battery_saver_threshold_percent: 20,
      network_latency_threshold_ms: 100,
    };
  }
  return invoke<LocalAiPolicy>("local_ai_policy");
}

export async function saveLocalAiPolicy(policy: LocalAiPolicy): Promise<LocalAiPolicy> {
  if (!isTauriRuntime()) return policy;
  return invoke<LocalAiPolicy>("save_local_ai_policy", { policy });
}

export async function disableStartupApp(name: string, location?: string | null): Promise<OptimizationResult> {
  if (!isTauriRuntime()) throw new Error("Tauri runtime unavailable for startup actions.");
  return invoke<OptimizationResult>("disable_startup_app", { name, location });
}

export async function restoreStartupApp(name?: string | null): Promise<OptimizationResult> {
  if (!isTauriRuntime()) throw new Error("Tauri runtime unavailable for startup actions.");
  return invoke<OptimizationResult>("restore_startup_app", { name });
}

export async function stopWindowsService(name: string): Promise<OptimizationResult> {
  if (!isTauriRuntime()) throw new Error("Tauri runtime unavailable for service actions.");
  return invoke<OptimizationResult>("stop_windows_service", { name });
}

export async function restoreWindowsService(name?: string | null): Promise<OptimizationResult> {
  if (!isTauriRuntime()) throw new Error("Tauri runtime unavailable for service actions.");
  return invoke<OptimizationResult>("restore_windows_service", { name });
}

export async function setPowerPlanHighPerformance(): Promise<OptimizationResult> {
  if (!isTauriRuntime()) throw new Error("Tauri runtime unavailable for power actions.");
  return invoke<OptimizationResult>("set_power_plan_high_performance");
}

export async function setPowerPlanBalanced(): Promise<OptimizationResult> {
  if (!isTauriRuntime()) throw new Error("Tauri runtime unavailable for power actions.");
  return invoke<OptimizationResult>("set_power_plan_balanced");
}

export async function setPowerPlanPowerSaver(): Promise<OptimizationResult> {
  if (!isTauriRuntime()) throw new Error("Tauri runtime unavailable for power actions.");
  return invoke<OptimizationResult>("set_power_plan_power_saver");
}

export async function setAgentTelemetryMode(mode: "normal" | "realtime") {
  if (!isTauriRuntime()) return { ...fallbackStatus, mode };
  return invoke<AgentStatus>("set_telemetry_mode", { mode });
}

export async function logoutAgent() {
  if (!isTauriRuntime()) return fallbackStatus;
  return invoke<AgentStatus>("logout");
}

export async function collectAgentTelemetrySample(): Promise<AgentTelemetrySample> {
  if (!isTauriRuntime()) throw new Error("Tauri runtime unavailable for local telemetry.");
  return invoke<AgentTelemetrySample>("collect_once");
}

export async function getAgentTelemetrySnapshot(): Promise<AgentTelemetrySnapshot | null> {
  if (!isTauriRuntime()) return null;
  return invoke<AgentTelemetrySnapshot | null>("telemetry_snapshot");
}

export async function listenToAgentTelemetry(onSnapshot: (snapshot: AgentTelemetrySnapshot) => void) {
  if (!isTauriRuntime()) return () => undefined;
  return listen<AgentTelemetrySnapshot>("telemetry-update", (event) => onSnapshot(event.payload));
}

export async function listenToAgentSessionInvalidated(onInvalidated: () => void) {
  if (!isTauriRuntime()) return () => undefined;
  return listen("agent-session-invalidated", onInvalidated);
}

export async function registerDeepLinkHandlers(onUrl: (url: string) => void) {
  if (!isTauriRuntime()) return () => undefined;

  const disposers: Array<() => void> = [];
  const handleUrls = (urls: Iterable<string>) => {
    for (const url of urls) {
      if (isAuthDeepLink(url)) onUrl(url);
    }
  };

  try {
    const urls = await getCurrent();
    handleUrls(urls ?? []);
  } catch {
    // Deep link startup payload is optional.
  }

  try {
    disposers.push(
      await onOpenUrl((urls) => {
        handleUrls(urls);
      }),
    );
  } catch {
    // Runtime deep link events are unavailable in some dev environments.
  }

  try {
    disposers.push(
      await listen<SingleInstancePayload>("single-instance", (event) => {
        handleUrls(extractDeepLinks(event.payload?.args));
      }),
    );
  } catch {
    // The single-instance plugin is a fallback for platforms that pass the URL as an argv.
  }

  return () => {
    for (const dispose of disposers) dispose();
  };
}

function extractDeepLinks(args: unknown[] | undefined) {
  if (!Array.isArray(args)) return [];
  return args.filter((value): value is string => typeof value === "string" && isAuthDeepLink(value));
}

function isAuthDeepLink(rawUrl: string) {
  try {
    const url = new URL(rawUrl);
    return url.protocol === "analystblaze:" && (url.hostname === "auth" || url.pathname.replace(/^\/+|\/+$/g, "") === "auth");
  } catch {
    return false;
  }
}
