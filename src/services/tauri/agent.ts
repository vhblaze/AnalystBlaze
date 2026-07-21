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
  billing_url: string;
  insights_url: string;
  focus_session?: FocusSession | null;
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
  gpu_temperature_source?: string | null;
  gpu_temperature_methods?: Array<{
    source: string;
    label?: string | null;
    value_c?: number | null;
    available: boolean;
  }>;
  thermal_sensors?: HardwareSensorReading[];
  power_sensors?: HardwareSensorReading[];
  fan_sensors?: HardwareSensorReading[];
  thermal_state?: string;
  thermal_trend?: string;
  throttling_suspected?: boolean;
  watts?: number | null;
  cpu_watts?: number | null;
  gpu_watts?: number | null;
  estimated_kwh?: number | null;
  energy_confidence?: number;
  is_estimated?: boolean;
  energy_source?: string;
  power_profile?: string;
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

export type HardwareSensorReading = {
  source: string;
  sensor_type: string;
  hardware_type?: string | null;
  hardware_name?: string | null;
  identifier?: string | null;
  label?: string | null;
  value: number;
  unit: string;
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

export type RemoteCommandConfirmationRequest = {
  requestId: string;
  commandId: string;
  actionName: string;
  title: string;
  description: string;
  risk: string;
  snapshot: boolean;
  authorizationMode?: string | null;
  authorizationId?: string | null;
  contextKey?: string | null;
};

export type GameModeResult = {
  success: boolean;
  message: string;
  details: unknown;
  status: AgentStatus;
};

export type GameModeSession = {
  id: string;
  targetPid?: number | null;
  targetProcessName?: string | null;
  snapshotIds: string[];
  createdAt: number;
  restoredAt?: number | null;
  status: string;
  restoreReason?: string | null;
};

export type FocusSessionEffects = {
  suppressAgentNotifications: boolean;
  visualPollingMinIntervalSeconds: number;
  pauseHeavyScans: boolean;
  delayNonCriticalUploads: boolean;
  nonCriticalUploadDelaySeconds: number;
  backgroundQuietMode: boolean;
  reduceSecondaryProcesses: boolean;
  sessionTag: string;
};

export type FocusSession = {
  id: string;
  profile: "work" | "game" | "call" | "study" | "focus" | string;
  label: string;
  createdAt: number;
  expiresAt: number;
  status: string;
  restoreReason?: string | null;
  restoredAt?: number | null;
  snapshotIds: string[];
  effects: FocusSessionEffects;
  quietDetails: unknown;
};

export type FocusModeProfile = "work" | "game" | "call" | "study" | "focus";

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

export type UpdateStatus = {
  currentVersion: string;
  checking: boolean;
  installing: boolean;
  available: boolean;
  version?: string | null;
  notes?: string | null;
  pubDate?: string | null;
  minimumVersion?: string | null;
  mandatory: boolean;
  downloaded: boolean;
  lastCheckedAt?: number | null;
  lastError?: string | null;
  dismissedUntil?: number | null;
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
  running: boolean;
  version?: string | null;
  requiresUpdate: boolean;
  canRequestUac: boolean;
  supportedActions: string[];
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

export type NetworkAdapterSummary = {
  name: string;
  description?: string | null;
  status?: string | null;
};

export type EnergyDiagnostics = {
  active_scheme_guid?: string | null;
  active_scheme_name?: string | null;
  active_scheme_alias?: string | null;
  /** Windows 11's Settings > Power "Power mode" slider overlay, when
   * applicable - null on Windows 10. See os_version.rs/energy.rs. */
  active_overlay_scheme_alias?: string | null;
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
  agent_mode: "off" | "manual" | "automatic" | string;
  auto_game_mode: boolean;
  auto_pc_clean: boolean;
  auto_restore_game_mode: boolean;
  optimize_power_plan: boolean;
  safe_temp_cleanup: boolean;
  energy_estimation_enabled: boolean;
  thermal_analysis_enabled: boolean;
  manage_startup_apps: boolean;
  manage_services: boolean;
  reduce_background_processes: boolean;
  allow_automatic_sensitive_actions: boolean;
  require_confirmation_for_sensitive: boolean;
  max_risk: "safe" | "sensitive" | string;
  confirmed_game_apps: string[];
  game_min_confidence: number;
  game_cooldown_seconds: number;
  pc_clean_cooldown_seconds: number;
  cleanup_min_idle_seconds: number;
  cleanup_disk_threshold_percent: number;
  thermal_cpu_limit_c: number;
  thermal_gpu_limit_c: number;
  battery_saver_threshold_percent: number;
  network_latency_threshold_ms: number;
};

export type PerformanceReport = {
  id: string;
  deviceId?: string | null;
  generatedAt: number;
  mode: "baseline" | "after" | "quick" | string;
  overallScore: number;
  previousScore?: number | null;
  measuredGainPercent?: number | null;
  scoreDeltaPercent?: number | null;
  scoreDeltaPoints?: number | null;
  performanceChange?: "improved" | "regressed" | "stable" | "unknown" | string;
  scoreBreakdown: {
    bootStartup: number;
    background: number;
    memory: number;
    disk: number;
    network: number;
    energy: number;
    thermal: number;
    gaming?: number | null;
  };
  metrics: {
    cpuUsagePercent: number;
    gpuUsagePercent: number;
    ramUsagePercent: number;
    diskUsagePercent: number;
    latencyMs: number;
    jitterMs?: number | null;
    packetLossPercent?: number | null;
    activeProcesses: number;
    cleanupReclaimableBytes: number;
    startupApps: number;
    highImpactStartupApps: number;
    pendingSnapshots: number;
    powerPlan?: string | null;
    cpuTemperatureC?: number | null;
    gpuTemperatureC?: number | null;
    gameDetected: boolean;
    gameProcess?: string | null;
  };
  deltas: Array<{
    key: string;
    before?: number | null;
    after?: number | null;
    unit: string;
    direction: string;
  }>;
  actions: Array<{
    actionName: string;
    status: string;
    message: string;
    snapshotId?: string | null;
    reversible: boolean;
    impactScore: number;
  }>;
  bottlenecks: Array<{
    id: string;
    label: string;
    severity: string;
    score: number;
    metric?: string | null;
    recommendedAction?: string | null;
  }>;
  restoreSession?: {
    id: string;
    snapshotIds: string[];
    status: string;
    createdAt: number;
    restoredAt?: number | null;
  } | null;
  source: string;
  metricsVersion: string;
};

export type CleanupCategory = {
  id: string;
  label: string;
  reclaimableBytes: number;
  scannedPaths: string[];
  risk: string;
  requiresHelper: boolean;
  reversible: boolean;
  availableActions: string[];
  skippedReason?: string | null;
};

export type StartupImpact = {
  name: string;
  location: string;
  publisher?: string | null;
  commandPreview: string;
  impactScore: number;
  risk: string;
  recommendation: string;
  availableActions: string[];
};

export type PcCleanFastOptions = {
  includeStartup?: boolean;
  includeCleanup?: boolean;
  includeBackground?: boolean;
  includeNetwork?: boolean;
  includeGaming?: boolean;
};

type SingleInstancePayload = {
  args?: unknown[];
  cwd?: string;
};

const DEV_API_BASE_URL = "http://127.0.0.1:8000";
// TEMPORARY: see the matching comment in src-tauri/src/config.rs -
// api.analystblaze.com's custom domain is stuck on Railway's side, so this
// points at Railway's default domain directly until that's fixed.
const PROD_API_BASE_URL = "https://analystblaze-server-production.up.railway.app";
const DEV_WEB_LOGIN_URL = "http://localhost:3000/login";
const PROD_WEB_LOGIN_URL = "https://analystblaze.com/login";
const DEV_ACCOUNT_SETTINGS_URL = "http://localhost:3000/configuration";
const PROD_ACCOUNT_SETTINGS_URL = "https://analystblaze.com/configuration";
const DEV_BILLING_URL = "http://localhost:3000/billing";
const PROD_BILLING_URL = "https://analystblaze.com/billing";
const DEV_INSIGHTS_URL = "http://localhost:3000/insights";
const PROD_INSIGHTS_URL = "https://analystblaze.com/insights";

function resolvePublicEndpoint(
  name: string,
  rawValue: string | undefined,
  devDefault: string,
  productionDefault: string,
) {
  const value = rawValue?.trim() || (import.meta.env.DEV ? devDefault : productionDefault);
  return validatePublicEndpoint(name, value);
}

function validatePublicEndpoint(name: string, value: string) {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw new Error(`${name} invalida: ${value}`);
  }

  if (url.protocol === "https:") return value.replace(/\/+$/, "");
  if (url.protocol === "http:" && import.meta.env.DEV && isDevLoopbackHost(url.hostname)) {
    return value.replace(/\/+$/, "");
  }

  if (url.protocol === "http:") {
    throw new Error(`${name} insegura: producao exige https://; http:// so e permitido para localhost/127.0.0.1 em modo dev.`);
  }

  throw new Error(`${name} insegura: protocolo nao permitido (${url.protocol}).`);
}

function isDevLoopbackHost(hostname: string) {
  return hostname.toLowerCase() === "localhost" || hostname === "127.0.0.1";
}

const fallbackStatus: AgentStatus = {
  authenticated: false,
  registered: false,
  hw_id: null,
  user_name: null,
  user_email: null,
  plan: "starter",
  has_paid_plan: false,
  mode: "stopped",
  api_base_url: resolvePublicEndpoint(
    "VITE_ANALYSTBLAZE_API_URL",
    import.meta.env.VITE_ANALYSTBLAZE_API_URL,
    DEV_API_BASE_URL,
    PROD_API_BASE_URL,
  ),
  web_login_url: resolvePublicEndpoint(
    "VITE_ANALYSTBLAZE_WEB_LOGIN_URL",
    import.meta.env.VITE_ANALYSTBLAZE_WEB_LOGIN_URL,
    DEV_WEB_LOGIN_URL,
    PROD_WEB_LOGIN_URL,
  ),
  account_settings_url: resolvePublicEndpoint(
    "VITE_ANALYSTBLAZE_ACCOUNT_URL",
    import.meta.env.VITE_ANALYSTBLAZE_ACCOUNT_URL,
    DEV_ACCOUNT_SETTINGS_URL,
    PROD_ACCOUNT_SETTINGS_URL,
  ),
  billing_url: resolvePublicEndpoint(
    "VITE_ANALYSTBLAZE_BILLING_URL",
    import.meta.env.VITE_ANALYSTBLAZE_BILLING_URL,
    DEV_BILLING_URL,
    PROD_BILLING_URL,
  ),
  insights_url: resolvePublicEndpoint(
    "VITE_ANALYSTBLAZE_INSIGHTS_URL",
    import.meta.env.VITE_ANALYSTBLAZE_INSIGHTS_URL,
    DEV_INSIGHTS_URL,
    PROD_INSIGHTS_URL,
  ),
  focus_session: null,
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

export async function openAgentBilling() {
  if (!isTauriRuntime()) return fallbackStatus.billing_url;
  return invoke<string>("open_billing");
}

export async function openAgentInsights() {
  if (!isTauriRuntime()) return fallbackStatus.insights_url;
  return invoke<string>("open_web_insights");
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

const fallbackUpdateStatus: UpdateStatus = {
  currentVersion: "0.1.0",
  checking: false,
  installing: false,
  available: false,
  mandatory: false,
  downloaded: false,
};

export async function getUpdateStatus(): Promise<UpdateStatus> {
  if (!isTauriRuntime()) return fallbackUpdateStatus;
  return invoke<UpdateStatus>("update_status");
}

export async function checkForUpdate(): Promise<UpdateStatus> {
  if (!isTauriRuntime()) return fallbackUpdateStatus;
  return invoke<UpdateStatus>("check_for_update");
}

export async function applyUpdate(): Promise<UpdateStatus> {
  requireTauriRuntime("Instalacao de atualizacoes");
  return invoke<UpdateStatus>("apply_update");
}

export async function dismissUpdate(): Promise<UpdateStatus> {
  requireTauriRuntime("Adiar atualizacao");
  return invoke<UpdateStatus>("dismiss_update");
}

export async function listenToUpdateStatus(onStatus: (status: UpdateStatus) => void) {
  if (!isTauriRuntime()) return () => undefined;
  return listen<UpdateStatus>("update-status-changed", (event) => onStatus(event.payload));
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

export async function listNetworkAdapters(): Promise<NetworkAdapterSummary[]> {
  requireTauriRuntime("Lista de adaptadores de rede");
  return invoke<NetworkAdapterSummary[]>("list_network_adapters");
}

export async function flushDnsCache(): Promise<OptimizationResult> {
  requireTauriRuntime("Limpeza de cache DNS");
  return invoke<OptimizationResult>("flush_dns_cache");
}

export async function setDnsServers(
  adapterName: string,
  dnsServers: string[],
): Promise<OptimizationResult> {
  requireTauriRuntime("Alteracao de servidores DNS");
  return invoke<OptimizationResult>("set_dns_servers", { adapterName, dnsServers });
}

export async function resetWinsockCatalog(): Promise<OptimizationResult> {
  requireTauriRuntime("Reset do catalogo Winsock");
  return invoke<OptimizationResult>("reset_winsock_catalog");
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

export async function installPrivilegedHelper(): Promise<PrivilegedHelperStatus> {
  requireTauriRuntime("Instalacao do helper privilegiado");
  return invoke<PrivilegedHelperStatus>("install_privileged_helper");
}

export async function uninstallPrivilegedHelper(): Promise<PrivilegedHelperStatus> {
  requireTauriRuntime("Remocao do helper privilegiado");
  return invoke<PrivilegedHelperStatus>("uninstall_privileged_helper");
}

export async function restartPrivilegedHelper(): Promise<PrivilegedHelperStatus> {
  requireTauriRuntime("Reinicio do helper privilegiado");
  return invoke<PrivilegedHelperStatus>("restart_privileged_helper");
}

export async function deepCleanTemp(): Promise<OptimizationResult> {
  requireTauriRuntime("Limpeza profunda de TEMP");
  return invoke<OptimizationResult>("deep_clean_temp");
}

export async function purgeCleanupQuarantine(): Promise<OptimizationResult> {
  requireTauriRuntime("Purge da quarentena de limpeza");
  return invoke<OptimizationResult>("purge_cleanup_quarantine");
}

export async function restoreActiveGameMode(): Promise<RestoreReport> {
  requireTauriRuntime("Restauracao do Modo Gamer");
  return invoke<RestoreReport>("restore_active_game_mode");
}

export async function getActiveGameModeSession(): Promise<GameModeSession | null> {
  requireTauriRuntime("Sessao ativa do Modo Gamer");
  return invoke<GameModeSession | null>("active_game_mode_session");
}

export async function activateFocusMode(profile: FocusModeProfile = "focus", durationSeconds?: number | null): Promise<OptimizationResult> {
  requireTauriRuntime("Modo Foco");
  return invoke<OptimizationResult>("activate_focus_mode", { profile, durationSeconds });
}

export async function restoreFocusSession(): Promise<RestoreReport> {
  requireTauriRuntime("Restauracao do Modo Foco");
  return invoke<RestoreReport>("restore_focus_session");
}

export async function getActiveFocusSession(): Promise<FocusSession | null> {
  requireTauriRuntime("Sessao ativa do Modo Foco");
  return invoke<FocusSession | null>("active_focus_session");
}

export async function runPerformanceScan(mode: "baseline" | "after" | "quick" = "quick"): Promise<PerformanceReport> {
  requireTauriRuntime("Performance Scan");
  return invoke<PerformanceReport>("run_performance_scan", { mode });
}

export async function applyPcCleanFastProfile(options: PcCleanFastOptions = {}): Promise<OptimizationResult> {
  requireTauriRuntime("Perfil PC limpo/rapido");
  return invoke<OptimizationResult>("apply_pc_clean_fast_profile", {
    includeStartup: options.includeStartup ?? true,
    includeCleanup: options.includeCleanup ?? true,
    includeBackground: options.includeBackground ?? true,
    includeNetwork: options.includeNetwork ?? false,
    includeGaming: options.includeGaming ?? true,
  });
}

export async function restorePerformanceSession(sessionId?: string | null): Promise<OptimizationResult> {
  requireTauriRuntime("Restauracao da suite de performance");
  return invoke<OptimizationResult>("restore_performance_session", { sessionId });
}

export async function scanCleanupCategories(): Promise<CleanupCategory[]> {
  requireTauriRuntime("Categorias de limpeza");
  return invoke<CleanupCategory[]>("scan_cleanup_categories");
}

export async function applyCleanupCategory(category: string, mode?: "safe" | "deep_confirmed" | string): Promise<OptimizationResult> {
  requireTauriRuntime("Limpeza por categoria");
  return invoke<OptimizationResult>("apply_cleanup_category", { category, mode });
}

export async function scanStartupImpact(): Promise<StartupImpact[]> {
  requireTauriRuntime("Impacto de inicializacao");
  return invoke<StartupImpact[]>("scan_startup_impact");
}

export async function delayStartupApp(name: string, location?: string | null, delaySeconds = 120): Promise<OptimizationResult> {
  requireTauriRuntime("Inicializacao atrasada");
  return invoke<OptimizationResult>("delay_startup_app", { name, location, delaySeconds });
}

export async function restoreDelayedStartupApp(name?: string | null): Promise<OptimizationResult> {
  requireTauriRuntime("Restaurar inicializacao atrasada");
  return invoke<OptimizationResult>("restore_delayed_startup_app", { name });
}

export async function getLocalAiPolicy(): Promise<LocalAiPolicy> {
  if (!isTauriRuntime()) {
    return {
      enabled: false,
      agent_mode: "manual",
      auto_game_mode: true,
      auto_pc_clean: true,
      auto_restore_game_mode: true,
      optimize_power_plan: true,
      safe_temp_cleanup: true,
      energy_estimation_enabled: true,
      thermal_analysis_enabled: true,
      manage_startup_apps: false,
      manage_services: false,
      reduce_background_processes: false,
      allow_automatic_sensitive_actions: false,
      require_confirmation_for_sensitive: true,
      max_risk: "safe",
      confirmed_game_apps: [],
      game_min_confidence: 0.74,
      game_cooldown_seconds: 900,
      pc_clean_cooldown_seconds: 3600,
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

export async function applyVisualPerformanceMode(): Promise<OptimizationResult> {
  if (!isTauriRuntime()) throw new Error("Tauri runtime unavailable for visual performance actions.");
  return invoke<OptimizationResult>("apply_visual_performance_mode");
}

export async function restoreVisualEffects(): Promise<OptimizationResult> {
  if (!isTauriRuntime()) throw new Error("Tauri runtime unavailable for visual performance actions.");
  return invoke<OptimizationResult>("restore_visual_effects");
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

export async function fetchAuthenticatedInsights(acceptLanguage?: string): Promise<unknown> {
  requireTauriRuntime("Insights autenticados");
  return invoke<unknown>("fetch_authenticated_insights", { acceptLanguage });
}

export async function listenToAgentTelemetry(onSnapshot: (snapshot: AgentTelemetrySnapshot) => void) {
  if (!isTauriRuntime()) return () => undefined;
  return listen<AgentTelemetrySnapshot>("telemetry-update", (event) => onSnapshot(event.payload));
}

export async function listenToAgentSessionInvalidated(onInvalidated: () => void) {
  if (!isTauriRuntime()) return () => undefined;
  return listen("agent-session-invalidated", onInvalidated);
}

export async function listenToRemoteCommandConfirmation(
  onRequest: (request: RemoteCommandConfirmationRequest) => void,
) {
  if (!isTauriRuntime()) return () => undefined;
  return listen<RemoteCommandConfirmationRequest>("remote-command-confirmation-request", (event) => onRequest(event.payload));
}

export async function resolveRemoteCommandConfirmation(requestId: string, approved: boolean) {
  if (!isTauriRuntime()) return false;
  return invoke<boolean>("resolve_remote_command_confirmation", { requestId, approved });
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
