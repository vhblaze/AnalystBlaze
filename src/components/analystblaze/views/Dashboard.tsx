import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Activity,
  ArrowUpRight,
  Cpu,
  Gamepad2,
  Gauge,
  HardDrive,
  Lock,
  MemoryStick,
  MonitorPlay,
  PlugZap,
  ShieldCheck,
  Sparkles,
  Thermometer,
  Timer,
  Wifi,
} from "lucide-react";
import { TiltCard } from "../TiltCard";
import type { User } from "@/hooks/useAuth";
import {
  getActiveGameModeSession,
  isTauriRuntime,
  type AgentStatus,
  type AgentTelemetrySnapshot,
  type GameModeSession,
} from "@/services/tauri/agent";
import { useTelemetry } from "@/hooks/useTelemetry";
import { useI18n } from "@/i18n";

const HISTORY_LEN = 28;

export function Dashboard({
  user,
  status,
  telemetry,
  onStartAgent,
  onActivateGameMode,
  onRestoreGameMode,
  onApplyPcCleanFast,
  onOpenBilling,
  busy,
}: {
  user: User | null;
  status: AgentStatus | null;
  telemetry: AgentTelemetrySnapshot | null;
  onStartAgent: () => Promise<void>;
  onActivateGameMode: () => Promise<void>;
  onRestoreGameMode: () => Promise<void>;
  onApplyPcCleanFast: () => Promise<void>;
  onOpenBilling: () => Promise<void>;
  busy: boolean;
}) {
  const { t } = useI18n();
  const track = useTelemetry("dashboard");
  const [cpuHistory, setCpuHistory] = useState<number[]>([]);
  const [activeGameModeSession, setActiveGameModeSession] = useState<GameModeSession | null>(null);
  const isAuthenticated = Boolean(status?.authenticated);
  const isReady = Boolean(status?.authenticated && status.registered);
  const paidGameModeAllowed = Boolean(status?.authenticated && status.registered && status.has_paid_plan);
  const gameModeActive = paidGameModeAllowed && Boolean(activeGameModeSession);
  const isLive = Boolean(isReady && telemetry);
  const lastCheckSeconds = telemetry
    ? Math.max(0, Math.round(Date.now() / 1000 - telemetry.event_timestamp))
    : null;
  const statusTitle = !isAuthenticated
    ? t("agent.status.waitingLogin")
    : !isReady
      ? t("agent.status.hardwarePending")
      : isLive
        ? t("dashboard.operational")
        : t("dashboard.waitingTelemetry");

  useEffect(() => {
    if (!telemetry) return;
    setCpuHistory((current) => [...current.slice(-(HISTORY_LEN - 1)), telemetry.cpu_usage]);
  }, [telemetry]);

  const refreshGameModeSession = useCallback(async () => {
    if (!paidGameModeAllowed || !isTauriRuntime()) {
      setActiveGameModeSession(null);
      return;
    }
    try {
      setActiveGameModeSession(await getActiveGameModeSession());
    } catch {
      setActiveGameModeSession(null);
    }
  }, [paidGameModeAllowed]);

  useEffect(() => {
    void refreshGameModeSession();
  }, [refreshGameModeSession]);

  const handleGameModeClick = async () => {
    track("game_mode_clicked");
    if (!isReady || !paidGameModeAllowed || gameModeActive) return;
    await onActivateGameMode();
    await refreshGameModeSession();
  };

  const handleGameModeDeactivate = async () => {
    track("game_mode_deactivate_clicked");
    await onRestoreGameMode();
    setActiveGameModeSession(null);
    await refreshGameModeSession();
  };

  const handlePcCleanFastClick = async () => {
    track("pc_clean_fast_clicked");
    if (!isReady) return;
    await onApplyPcCleanFast();
  };

  const handleGameModeUpsellClick = async () => {
    track("game_mode_upsell_clicked");
    await onOpenBilling();
  };

  const healthLabel = useMemo(() => healthLevelLabel(telemetry?.health_level, t), [telemetry?.health_level, t]);

  return (
    <div className="flex flex-col gap-8">
      <header className="flex flex-col gap-2">
        <div className="flex items-center gap-2 font-mono text-[11px] uppercase tracking-[0.3em] text-cyan-400/70">
          <Sparkles className="h-3 w-3" />
          {t("dashboard.eyebrow")}
        </div>
        <h1 className="text-[42px] font-semibold leading-tight tracking-tight text-slate-50">
          {t("dashboard.greetingPrefix")}{" "}
          <span className="text-gradient-cyber">{user?.name ?? t("dashboard.fallbackPilot")}</span>
        </h1>
        <p className="max-w-xl text-sm text-slate-400">
          {telemetry ? t("dashboard.descriptionLive") : t("dashboard.description")}
        </p>
      </header>

      <TiltCard className="h-[320px]" intensity={5}>
        <div className="relative flex h-full flex-col justify-between overflow-hidden p-8">
          <div className="pointer-events-none absolute -right-16 -top-16 h-64 w-64 rounded-full bg-gradient-to-br from-cyan-500/30 via-violet-500/20 to-transparent blur-3xl" />
          <div className="pointer-events-none absolute -left-10 bottom-0 h-40 w-40 rounded-full bg-fuchsia-500/10 blur-3xl" />

          <div className="relative flex items-start justify-between gap-6">
            <div className="min-w-0">
              <div className="font-mono text-[11px] uppercase tracking-[0.3em] text-cyan-400/80">
                {t("dashboard.statusLabel")}
              </div>
              <div className="mt-3 flex flex-wrap items-center gap-3">
                <span className="relative flex h-3 w-3">
                  <span className={`absolute inline-flex h-full w-full animate-ping rounded-full ${isLive ? "bg-cyan-400" : "bg-amber-400"} opacity-60`} />
                  <span className={`relative inline-flex h-3 w-3 rounded-full ${isLive ? "bg-cyan-400 shadow-[0_0_12px_hsl(187_100%_60%)]" : "bg-amber-400 shadow-[0_0_12px_hsl(40_100%_60%)]"}`} />
                </span>
                <span className="text-3xl font-semibold tracking-tight text-slate-50">
                  {statusTitle}
                </span>
                <span className={`rounded-md border px-2 py-0.5 font-mono text-[10px] uppercase tracking-widest ${healthTone(telemetry?.health_level)}`}>
                  {telemetry ? healthLabel : t("common.pending")}
                </span>
              </div>
              <p className="mt-2 text-sm text-slate-400">
                {lastCheckSeconds === null
                  ? t("dashboard.noTelemetryDesc")
                  : t("dashboard.lastCheck", { seconds: lastCheckSeconds })}
              </p>
            </div>
            <div className="flex shrink-0 flex-col items-end gap-2">
              <div className="grid h-12 w-12 place-items-center rounded-xl border border-cyan-400/30 bg-cyan-500/10">
                <ShieldCheck className="h-6 w-6 text-cyan-300" />
              </div>
              <div className="font-mono text-[10px] uppercase tracking-[0.25em] text-slate-500">
                {telemetry?.telemetry_mode ?? status?.mode ?? t("common.stopped")}
              </div>
            </div>
          </div>

          <div className="relative grid grid-cols-3 gap-3">
            <Stat
              icon={<Gauge className="h-3.5 w-3.5" />}
              label={t("dashboard.healthScore")}
              value={telemetry ? `${telemetry.health_score}%` : "--"}
              delta={telemetry ? healthLabel : t("common.unavailable")}
              tone={telemetry?.health_level}
            />
            <Stat
              icon={<Cpu className="h-3.5 w-3.5" />}
              label={t("dashboard.cpuLoad")}
              value={telemetry ? formatPercent(telemetry.cpu_usage) : "--"}
              delta={telemetry ? t("dashboard.activeProfileValue", { profile: profileLabel(telemetry.active_profile, t) }) : t("common.unavailable")}
              tone={telemetry?.cpu_usage && telemetry.cpu_usage > 80 ? "watch" : "good"}
            />
            <Stat
              icon={<Activity className="h-3.5 w-3.5" />}
              label={t("dashboard.processes")}
              value={telemetry?.active_processes?.toString() ?? "--"}
              delta={telemetry ? formatDuration(telemetry.system_uptime_seconds) : t("common.unavailable")}
              tone="good"
            />
          </div>

          <div className="relative flex items-center justify-between gap-4">
            <div className="flex flex-col items-start gap-2">
              <div className="flex flex-wrap items-center gap-2">
                <button
                  disabled={busy || !isReady}
                  onClick={() => {
                    if (!isReady) {
                      void onStartAgent();
                      return;
                    }
                    if (gameModeActive) {
                      void handleGameModeDeactivate();
                      return;
                    }
                    if (!paidGameModeAllowed) {
                      void handleGameModeUpsellClick();
                      return;
                    }
                    void handleGameModeClick();
                  }}
                  className={`group inline-flex items-center gap-2.5 rounded-xl border px-6 py-3 text-sm font-semibold transition-all duration-300 disabled:opacity-70 ${
                    gameModeActive
                      ? "border-emerald-300/50 bg-emerald-400/15 text-emerald-50 hover:border-amber-300/60 hover:bg-amber-400/15"
                      : "border-cyan-400/40 bg-gradient-to-r from-cyan-500/20 to-violet-500/10 text-cyan-100 hover:border-cyan-300/60 hover:from-cyan-500/30 hover:shadow-[0_0_30px_-5px_hsl(187_100%_55%/0.7)]"
                  }`}
                >
                  {gameModeActive ? (
                    <ShieldCheck className="h-4 w-4" />
                  ) : isReady && !paidGameModeAllowed ? (
                    <Lock className="h-4 w-4" />
                  ) : (
                    <Gamepad2 className="h-4 w-4 transition-transform group-hover:-rotate-12" />
                  )}
                  {isReady
                    ? gameModeActive
                      ? "Desativar Modo Gamer"
                      : paidGameModeAllowed
                        ? t("dashboard.gameMode")
                        : "Desbloquear Modo Gamer"
                    : t("dashboard.startAgent")}
                  {!gameModeActive && <ArrowUpRight className="h-4 w-4 opacity-60 transition-transform group-hover:translate-x-0.5 group-hover:-translate-y-0.5" />}
                </button>
                <button
                  disabled={busy || !isReady}
                  onClick={() => void handlePcCleanFastClick()}
                  className="group inline-flex items-center gap-2.5 rounded-xl border border-emerald-400/40 bg-emerald-400/10 px-5 py-3 text-sm font-semibold text-emerald-100 transition-all duration-300 hover:border-emerald-300/60 hover:bg-emerald-400/15 disabled:opacity-50"
                >
                  <Sparkles className="h-4 w-4" />
                  PC limpo/rapido
                </button>
              </div>
              {isReady && !paidGameModeAllowed && (
                <span className="text-xs text-slate-500">Abre a pagina de planos</span>
              )}
            </div>
            <Sparkline data={cpuHistory} />
          </div>
        </div>
      </TiltCard>

      <div className="grid grid-cols-1 gap-5 md:grid-cols-3">
        <MetricCard icon={MemoryStick} label={t("dashboard.ramLoad")} value={telemetry ? formatPercent(telemetry.ram_usage_percent) : "--"} detail={telemetry ? `${formatMb(telemetry.ram_usage_mb)} / ${formatMb(telemetry.ram_total_mb ?? 0)}` : t("common.unavailable")} />
        <MetricCard icon={MonitorPlay} label={t("dashboard.gpu")} value={telemetry?.gpu_name || "--"} detail={telemetry?.gpu_usage_available ? `${formatPercent(telemetry.gpu_usage)} ${t("dashboard.gpuLoad")}` : t("dashboard.gpuLoadUnavailable")} />
        <MetricCard icon={Thermometer} label={t("dashboard.cpuTemp")} value={formatTemp(telemetry?.cpu_temperature, telemetry?.cpu_temperature_available)} detail={telemetry ? thermalDetail(telemetry) : t("common.unavailable")} />
        <MetricCard icon={Thermometer} label={t("dashboard.gpuTemp")} value={formatTemp(telemetry?.gpu_temperature, telemetry?.gpu_temperature_available)} detail={telemetry ? `${formatGb(telemetry.vram_gb)} ${t("dashboard.vramTotal")} / ${thermalStateLabel(telemetry.thermal_state)}` : t("common.unavailable")} />
        <MetricCard icon={PlugZap} label="Energia" value={formatWatts(telemetry?.watts)} detail={telemetry ? energyDetail(telemetry) : t("common.unavailable")} />
        <MetricCard icon={HardDrive} label={t("dashboard.diskUsage")} value={telemetry ? formatPercent(telemetry.disk_usage_percent ?? 0) : "--"} detail={telemetry ? `${formatGb(telemetry.disk_used_gb ?? 0)} / ${formatGb(telemetry.disk_total_gb ?? 0)}` : t("common.unavailable")} />
        <MetricCard icon={Wifi} label={t("dashboard.latency")} value={telemetry ? formatLatency(telemetry.latency_ms) : "--"} detail={telemetry ? networkDetail(telemetry.network) : t("common.unavailable")} />
        <MetricCard icon={Timer} label={t("dashboard.idleState")} value={telemetry ? formatDuration(telemetry.idle_seconds ?? 0) : "--"} detail={telemetry?.active_window || t("dashboard.noActiveWindow")} />
        <MetricCard icon={ShieldCheck} label={t("dashboard.optimizationStatus")} value={telemetry ? optimizationLabel(telemetry.optimization_status, t) : "--"} detail={telemetry ? profileLabel(telemetry.active_profile, t) : t("common.unavailable")} />
      </div>
    </div>
  );
}

function Stat({
  icon,
  label,
  value,
  delta,
  tone,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  delta: string;
  tone?: string;
}) {
  return (
    <div className="rounded-xl border border-cyan-500/10 bg-slate-950/50 p-4 backdrop-blur-sm">
      <div className="flex items-center justify-between gap-2 text-slate-500">
        <div className="flex min-w-0 items-center gap-1.5 font-mono text-[10px] uppercase tracking-widest">
          {icon}
          <span className="truncate">{label}</span>
        </div>
        <span className={`truncate font-mono text-[10px] ${textTone(tone)}`}>
          {delta}
        </span>
      </div>
      <div className="mt-2 truncate text-2xl font-semibold tracking-tight text-slate-50">{value}</div>
    </div>
  );
}

function MetricCard({
  icon: Icon,
  label,
  value,
  detail,
}: {
  icon: typeof Activity;
  label: string;
  value: string;
  detail: string;
}) {
  return (
    <div className="rounded-2xl border border-cyan-500/10 bg-slate-950/45 p-5">
      <div className="flex items-center gap-2 font-mono text-[10px] uppercase tracking-[0.25em] text-slate-500">
        <Icon className="h-4 w-4 text-cyan-300" />
        <span className="truncate">{label}</span>
      </div>
      <div className="mt-3 truncate text-xl font-semibold text-slate-100">{value}</div>
      <div className="mt-1 truncate text-xs text-slate-500">{detail}</div>
    </div>
  );
}

function Sparkline({ data }: { data: number[] }) {
  const points = useMemo(() => {
    if (data.length < 2) return "";
    const width = 120;
    const height = 36;
    return data
      .map((value, index) => {
        const x = (index / (data.length - 1)) * width;
        const y = height - (Math.max(0, Math.min(100, value)) / 100) * height;
        return `${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(" ");
  }, [data]);

  return (
    <svg viewBox="0 0 120 36" className="h-9 w-32 opacity-90">
      <defs>
        <linearGradient id="sparkGrad" x1="0" x2="1" y1="0" y2="0">
          <stop offset="0%" stopColor="hsl(187 100% 60%)" />
          <stop offset="100%" stopColor="hsl(265 90% 70%)" />
        </linearGradient>
      </defs>
      {points ? (
        <polyline points={points} fill="none" stroke="url(#sparkGrad)" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
      ) : (
        <line x1="0" y1="30" x2="120" y2="30" stroke="hsl(215 20% 35%)" strokeDasharray="3 5" strokeWidth="1.2" />
      )}
    </svg>
  );
}

function formatPercent(value: number) {
  return `${Math.round(Math.max(0, Math.min(100, value)))}%`;
}

function formatLatency(value?: number | null) {
  if (value == null || !Number.isFinite(value) || value <= 0) return "--";
  return `${Math.round(value)} ms`;
}

function networkDetail(network: unknown) {
  if (!network || typeof network !== "object") return "rede em leitura";
  const data = network as {
    adapter_name?: string | null;
    wifi_ssid?: string | null;
    jitter_ms?: number | null;
    packet_loss_percent?: number | null;
  };
  const name = data.wifi_ssid || data.adapter_name || "adaptador ativo";
  const loss = data.packet_loss_percent != null ? `${Math.round(data.packet_loss_percent)}% perda` : null;
  const jitter = data.jitter_ms != null ? `${Math.round(data.jitter_ms)} ms jitter` : null;
  return [name, loss, jitter].filter(Boolean).join(" - ");
}

function formatGb(value: number) {
  if (!Number.isFinite(value) || value <= 0) return "0 GB";
  return `${value.toFixed(value >= 10 ? 0 : 1)} GB`;
}

function formatMb(value: number) {
  if (!Number.isFinite(value) || value <= 0) return "0 MB";
  if (value >= 1024) return formatGb(value / 1024);
  return `${Math.round(value)} MB`;
}

function formatTemp(value: number | undefined, available: boolean | undefined) {
  if (!available || typeof value !== "number" || !Number.isFinite(value)) return "--";
  return `${Math.round(value)} C`;
}

function formatWatts(value?: number | null) {
  if (typeof value !== "number" || !Number.isFinite(value) || value <= 0) return "--";
  return `${Math.round(value)} W`;
}

function thermalDetail(telemetry: AgentTelemetrySnapshot) {
  const source = telemetry.cpu_temperature_source ? temperatureSourceLabel(telemetry.cpu_temperature_source) : "sensor indisponivel";
  const sensors = telemetry.thermal_sensors?.length ? `${telemetry.thermal_sensors.length} sensores` : source;
  return `${thermalStateLabel(telemetry.thermal_state)} / ${trendLabel(telemetry.thermal_trend)} / ${sensors}`;
}

function energyDetail(telemetry: AgentTelemetrySnapshot) {
  const confidence = typeof telemetry.energy_confidence === "number"
    ? `${Math.round(telemetry.energy_confidence * 100)}%`
    : "--";
  const source = telemetry.is_estimated ? "estimado" : "sensor";
  const power = telemetry.power_sensors?.length ? ` / ${telemetry.power_sensors.length} sensores W` : "";
  return `${source}${power} / ${confidence} confianca / ${telemetry.power_profile ?? "perfil atual"}`;
}

function thermalStateLabel(value?: string) {
  if (value === "critical") return "critico";
  if (value === "hot") return "quente";
  if (value === "watch") return "atencao";
  if (value === "normal") return "normal";
  return "sem leitura";
}

function trendLabel(value?: string) {
  if (value === "rising") return "subindo";
  if (value === "falling") return "caindo";
  if (value === "stable") return "estavel";
  if (value === "warming_up") return "aquecendo";
  return "tendencia indisponivel";
}

function temperatureSourceLabel(source: string) {
  if (source === "nvml") return "NVML";
  if (source === "libre_hardware_monitor") return "LibreHardwareMonitor";
  if (source === "open_hardware_monitor") return "OpenHardwareMonitor";
  if (source === "acpi_thermal_zone") return "ACPI";
  if (source === "sysinfo_cpu_sensor") return "sysinfo";
  if (source === "sysinfo_gpu_sensor") return "sysinfo GPU";
  if (source === "sysinfo_component_max") return "sysinfo max";
  if (source === "hardware_monitor") return "monitor";
  return source;
}

function formatDuration(seconds: number) {
  if (!Number.isFinite(seconds) || seconds < 60) return `${Math.max(0, Math.round(seconds || 0))}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return `${hours}h`;
  return `${Math.floor(hours / 24)}d`;
}

function healthTone(level?: string) {
  if (level === "critical") return "border-rose-400/30 bg-rose-400/10 text-rose-300";
  if (level === "watch") return "border-amber-400/30 bg-amber-400/10 text-amber-300";
  return "border-emerald-400/30 bg-emerald-400/10 text-emerald-300";
}

function textTone(level?: string) {
  if (level === "critical") return "text-rose-400";
  if (level === "watch") return "text-amber-400";
  return "text-emerald-400";
}

function healthLevelLabel(level: string | undefined, t: (key: string, params?: Record<string, string | number | boolean>) => string) {
  if (!level) return t("common.unavailable");
  return t(`dashboard.health.${level}`);
}

function profileLabel(profile: string, t: (key: string, params?: Record<string, string | number | boolean>) => string) {
  return t(`dashboard.profiles.${profile}`);
}

function optimizationLabel(status: string, t: (key: string, params?: Record<string, string | number | boolean>) => string) {
  return t(`dashboard.optimization.${status}`);
}
