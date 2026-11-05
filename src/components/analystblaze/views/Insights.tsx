import { useEffect, useMemo, useState } from "react";
import { AlertCircle, ArrowRight, Bot, Brain, Cpu, Droplets, ExternalLink, RefreshCw, Sparkles, User, Wind, X, Zap } from "lucide-react";
import { fetchInsights, type DiskNearFullInfo, type Insight } from "@/services/insights";
import { useI18n } from "@/i18n";
import { useTelemetry } from "@/hooks/useTelemetry";
import {
  checkDiskOptimizationInsight,
  isTauriRuntime,
  openAgentInsights,
  openWindowsSecurity,
  type AgentTelemetrySnapshot,
  type DiskOptimizationInsight,
  type NetworkDiagnostics,
} from "@/services/tauri/agent";
import { formatBytes } from "@/lib/format";
import { networkDiagnostics as networkDiagnosticsResource, useNetwork } from "@/features/network/useNetwork";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { windowsHealthItems } from "@/features/health/windowsDiagnostics";

const DISK_USAGE_WARNING_THRESHOLD_PERCENT = 80;
const NETWORK_LAG_FLAGS = ["packet_loss_detected", "jitter_high", "latency_high"] as const;
const LATENCY_THRESHOLD_MS = 90;
const INSIGHT_CONFIDENCE_THRESHOLD = 0.5;
// A game-server reading only means something once it clears both bars: an
// absolute gap (a slow connection to a nearby target can still be +30ms
// worse just from noise) and a relative one (a target that's already at
// 80ms doesn't need to double to be worth flagging the same way a 10ms one
// does). Both must hold before this is treated as evidence of anything.
const GAME_SERVER_LATENCY_GAP_MS = 60;
const GAME_SERVER_LATENCY_RATIO = 2;
/** DiskExplorer only reports a volume here once it's already over its own
 * (higher) threshold - this is just for confidence scaling, not a second
 * gate. */
const DISK_NEAR_FULL_BASE_PERCENT = 90;

const DISMISS_STORAGE_KEY = "analystblaze.dismissedInsights";
const DISMISS_TTL_MS = 24 * 60 * 60 * 1000;
/** No máximo 3 sugestões por vez; as outras ficam em "ver todas" (doc 11 §7). */
const MAX_VISIBLE = 3;
/** The server only ever pairs a card with an actionName from its own small
 * validated allowlist (see safe_action_policy.py) - these are the ones the
 * desktop already knows how to run locally, so "I'll do it myself" has
 * something real to call. Anything else server-side is display-only for now. */
const LOCALLY_EXECUTABLE_ACTIONS = new Set([
  "APPLY_GAME_MODE",
  "EMPTY_TEMP",
  "ENABLE_SCHEDULED_DEFRAG",
  "START_SYSTEM_FILE_CHECK",
  "RESTART_PNP_DEVICE",
  "DISABLE_GAME_DVR",
  "REPAIR_SERVICE",
  "DISABLE_SERVICE_PERMANENTLY",
  "THROTTLE_DEFENDER_SCANS",
  "RESTORE_DEFENDER_SCAN_SETTINGS",
  "RENEW_DEFENDER_DEFINITIONS",
  "SCHEDULE_VOLUME_CHECK",
]);
/** Actions that only ever run on the user's own machine, never queued to
 * the server - either because the action isn't (yet) registered in the
 * server's SUPPORTED_REMOTE_ACTIONS/AGENT_COMMAND_ALLOWLIST, or, as with
 * START_SYSTEM_FILE_CHECK, deliberately: sfc/DISM already run entirely
 * through the local privileged helper, so there's nothing for the server
 * to add - a network round-trip and server load for zero benefit. For
 * these, "Deixar o agente fazer" still shows (the local AnalystBlaze agent
 * is still "the agent"), but routes to the same local call as "Fazer eu
 * mesmo" instead of POSTing to /insights/actions - see requestAgentApply. */
const LOCAL_ONLY_ACTIONS = new Set([
  "START_SYSTEM_FILE_CHECK",
  "RESTART_PNP_DEVICE",
  "DISABLE_GAME_DVR",
  "REPAIR_SERVICE",
  "DISABLE_SERVICE_PERMANENTLY",
  "THROTTLE_DEFENDER_SCANS",
  "RESTORE_DEFENDER_SCAN_SETTINGS",
  "RENEW_DEFENDER_DEFINITIONS",
  "SCHEDULE_VOLUME_CHECK",
]);
/** Actions whose useAuth wrapper resolves with the real outcome instead of
 * throwing on failure (see restartPnpDevice's comment in useAuth.ts) -
 * "didn't fix it" is legitimate information to show, not an exception, so
 * these only dismiss the card when the outcome actually says success. */
const HONEST_OUTCOME_ACTIONS = new Set([
  "RESTART_PNP_DEVICE",
  "REPAIR_SERVICE",
  "DISABLE_SERVICE_PERMANENTLY",
  "THROTTLE_DEFENDER_SCANS",
  "RESTORE_DEFENDER_SCAN_SETTINGS",
  "RENEW_DEFENDER_DEFINITIONS",
  "SCHEDULE_VOLUME_CHECK",
]);
/** How many Critical-level (level 1) Windows Event Log entries in the last
 * 24h are worth mentioning. This is deliberately conservative and paired
 * with "pode (não necessariamente) indicar" wording below, not "seu Windows
 * esta com problema" - a healthy PC can log a handful of criticals from
 * completely unrelated one-off causes, so this is a prompt to check, not a
 * diagnosis. */
const EVENT_LOG_CRITICAL_THRESHOLD = 10;
// Human labels for the Win32_PnPEntity ConfigManagerErrorCode values the
// backend already filters to (see telemetry::advanced::PROBLEM_DEVICE_CODES) -
// matches the Device Manager wording users may already be familiar with.
const PROBLEM_CODE_LABELS: Record<number, string> = {
  10: "Código 10 - o dispositivo não consegue iniciar",
  12: "Código 12 - recursos de hardware insuficientes",
  14: "Código 14 - precisa reiniciar o Windows para funcionar",
  18: "Código 18 - drivers precisam ser reinstalados",
  19: "Código 19 - configuração do Registro corrompida",
  28: "Código 28 - drivers não instalados",
  31: "Código 31 - o Windows não conseguiu iniciar o dispositivo",
  37: "Código 37 - o driver retornou um erro",
  38: "Código 38 - uma instancia anterior do driver ainda está carregada",
  39: "Código 39 - driver corrompido ou ausente",
  40: "Código 40 - entrada do Registro do driver corrompida",
  43: "Código 43 - o Windows parou o dispositivo por um problema reportado",
  48: "Código 48 - driver bloqueado por incompatibilidade",
};

/** Mirrors telemetry::modem_link::ModemUsbLinkIssue on the Rust side. */
type ModemUsbLinkIssue = {
  usb_device_id: string;
  usb_device_name?: string | null;
  link: {
    port?: number | null;
    on_root_hub: boolean;
    hub_version?: string | null;
    controller?: string | null;
  };
  root_net_device_id?: string | null;
  ghost_wwan_adapter_count: number;
};

/** The one thing the modem-link diagnosis can't read off the machine: whether
 * the person actually has a 4G/5G modem plugged in. Answered once, remembered
 * (a wrong guess here flips the whole recommendation - "fix your USB link" vs
 * "remove a leftover adapter"), and resettable from the card itself. */
const MODEM_ANSWER_STORAGE_KEY = "analystblaze.modemUsbAnswer";
type ModemAnswer = "yes" | "no";

function loadModemAnswer(): ModemAnswer | null {
  try {
    const raw = localStorage.getItem(MODEM_ANSWER_STORAGE_KEY);
    return raw === "yes" || raw === "no" ? raw : null;
  } catch {
    return null;
  }
}

/** Mirrors telemetry::defender_disk::DefenderDiskIssue on the Rust side. */
type DefenderDiskCause =
  | "disk_failing"
  | "file_system_corruption"
  | "threat_remediation_loop"
  | "scan_cannot_finish"
  | "definitions_or_engine_failing"
  | "unthrottled_scan"
  | "unknown";
type DefenderDiskIssue = {
  cause: DefenderDiskCause;
  detected_at: number;
  disk?: string | null;
  disk_number?: number | null;
  sustained_minutes: number;
  peak_disk_active_percent: number;
  peak_defender_mb_s: number;
  disk_error_events_7d: number;
  ntfs_corruption_events_7d: number;
  unhealthy_volumes: string[];
  detections_24h: number;
  remediation_failures_24h: number;
  scans_started_24h: number;
  scans_finished_24h: number;
  scans_failed_24h: number;
  signature_update_failures_24h: number;
  engine_failures_24h: number;
  scan_avg_cpu_load_factor?: number | null;
  disable_cpu_throttle_on_idle_scans?: boolean | null;
  scan_only_if_idle?: boolean | null;
  full_scan_age_days?: number | null;
};
/** Set when THROTTLE_DEFENDER_SCANS succeeds, cleared when the restore does -
 * keeps a small "restaurar" card around so the change is never a one-way
 * door the user has to dig through the action history to undo. */
const DEFENDER_THROTTLE_STORAGE_KEY = "analystblaze.defenderScansThrottledAt";

function loadDefenderThrottledAt(): number | null {
  try {
    const raw = localStorage.getItem(DEFENDER_THROTTLE_STORAGE_KEY);
    const value = raw ? Number(raw) : NaN;
    return Number.isFinite(value) ? value : null;
  } catch {
    return null;
  }
}

/** `1 D: E:` -> "D": the first drive letter on a PDH PhysicalDisk instance. */
function firstDriveLetter(instance: string | null | undefined): string | null {
  const match = /\b([A-Za-z]):/.exec(instance ?? "");
  return match ? match[1].toUpperCase() : null;
}

function insightKey(insight: Pick<Insight, "category" | "actionName" | "title">): string {
  // Includes title even when actionName is set - multiple failing devices
  // all share actionName "RESTART_PNP_DEVICE" (only actionContext.deviceId
  // differs), so actionName alone would collapse them onto one dismiss key.
  return `${insight.category}:${insight.actionName ?? insight.title}:${insight.title}`;
}

function loadDismissed(): Record<string, number> {
  try {
    const raw = JSON.parse(localStorage.getItem(DISMISS_STORAGE_KEY) ?? "{}");
    const now = Date.now();
    const fresh: Record<string, number> = {};
    for (const [key, dismissedAt] of Object.entries(raw)) {
      if (typeof dismissedAt === "number" && now - dismissedAt < DISMISS_TTL_MS) {
        fresh[key] = dismissedAt;
      }
    }
    return fresh;
  } catch {
    return {};
  }
}

type Category = "performance" | "energia" | "rede" | "limpeza";

const meta: Record<Category, { icon: React.ComponentType<{ className?: string }>; tone: string; ring: string; chip: string }> = {
  performance: {
    icon: Zap,
    tone: "from-accent-strong/15 to-accent-strong/0",
    ring: "border-line",
    chip: "border-accent/30 bg-accent-strong/10 text-accent-soft",
  },
  energia: {
    icon: Cpu,
    tone: "from-warn-strong/15 to-warn-strong/0",
    ring: "border-warn/25",
    chip: "border-warn/30 bg-warn-strong/10 text-warn-soft",
  },
  rede: {
    icon: Wind,
    tone: "from-violet-strong/15 to-violet-strong/0",
    ring: "border-violet/25",
    chip: "border-violet/30 bg-violet-strong/10 text-violet-soft",
  },
  limpeza: {
    icon: Droplets,
    tone: "from-ok-strong/15 to-ok-strong/0",
    ring: "border-ok/25",
    chip: "border-ok/30 bg-ok-strong/10 text-ok-soft",
  },
};

export function Insights({
  telemetry,
  diskNearFullInfo,
  onOpenDiskUsage,
  onOpenNetwork,
  onApplyInsightActionLocally,
  onRequestAgentApplyInsight,
  shadowConsentNeeded,
  onOpenWindowsHealth,
}: {
  telemetry?: AgentTelemetrySnapshot | null;
  /** Set by DiskExplorer, on demand, the moment it detects some individual
   * volume near capacity - null means either nothing's near full or the
   * user hasn't opened Disk Explorer yet this session (no background scan
   * exists to fill this in ahead of time). */
  diskNearFullInfo?: DiskNearFullInfo | null;
  onOpenDiskUsage?: () => void;
  /** Optional target: when passed (see gameServerLatencyInsight), Network
   * jumps straight to the route tab and runs a traceroute against it
   * instead of opening on the general diagnostics tab. */
  onOpenNetwork?: (autoTracerouteTarget?: string) => void;
  /** "I'll do it myself" - runs the action right now, locally, with the
   * same confirmation dialog its dedicated button elsewhere already uses.
   * `context` carries per-instance data an actionName alone can't (e.g.
   * which of several failing devices RESTART_PNP_DEVICE should target). */
  onApplyInsightActionLocally?: (actionName: string, context?: Record<string, unknown>) => Promise<unknown>;
  /** "Let the agent do it" - enqueues the action server-side; the agent
   * applies it on its own next sync cycle (see applyInsightAction). */
  onRequestAgentApplyInsight?: (actionName: string, title: string, reason: string) => Promise<unknown>;
  /** Shadow-copy storage hit its limit and the user hasn't yet chosen
   * whether AnalystBlaze may raise the cap for them. Surfaces here as a
   * two-choice card instead of a modal (see AppShell). */
  shadowConsentNeeded?: boolean;
  onResolveShadowConsent?: (choice: "auto" | "manual") => Promise<void>;
  /** Saúde do Windows mora em Meu PC; aqui fica só o resumo com o link. */
  onOpenWindowsHealth?: () => void;
}) {
  const { t, locale } = useI18n();
  const track = useTelemetry("insights");
  const [insights, setInsights] = useState<Insight[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState<Record<string, number>>(() => loadDismissed());
  const [actionBusyKey, setActionBusyKey] = useState<string | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [showAll, setShowAll] = useState(false);
  // Only fetched here, on demand, for the local VPN+latency insight below -
  // the passive telemetry sample doesn't probe the adapter/VPN detection
  // (kept out of the 2s telemetry loop deliberately, see collect_network_sample).
  // Mesma leitura que Meu PC › Rede usa (features/network).
  const networkDiagnostics = useNetwork({ auto: false }).diagnostics;
  // Same independent-fetch pattern as networkDiagnostics above, not shared
  // state with DiskExplorer (which fetches this too, for its own card) -
  // cheap enough (a couple of unelevated PowerShell calls) that duplicating
  // it here is simpler than lifting shared state up.
  const [diskOptimizationInsightData, setDiskOptimizationInsightData] = useState<DiskOptimizationInsight | null>(null);

  const dismissInsight = (insight: Insight) => {
    const key = insightKey(insight);
    track("insight_dismissed", { key });
    setDismissed((current) => {
      const next = { ...current, [key]: Date.now() };
      try {
        localStorage.setItem(DISMISS_STORAGE_KEY, JSON.stringify(next));
      } catch {
        // Non-critical preference persistence.
      }
      return next;
    });
  };

  const applyAction = async (insight: Insight, actionName: string, actionContext: Record<string, unknown> | undefined, busyKey: string) => {
    if (!onApplyInsightActionLocally) return;
    setActionBusyKey(busyKey);
    setActionMessage(null);
    try {
      const result = await onApplyInsightActionLocally(actionName, actionContext);
      track("insight_action_applied_locally", { actionName });
      if (actionName === "START_SYSTEM_FILE_CHECK") {
        setActionMessage("Verificação iniciada em segundo plano. Acompanhe o progresso em Meu PC › Saúde do Windows.");
        dismissInsight(insight);
      } else if (HONEST_OUTCOME_ACTIONS.has(actionName)) {
        const outcome = result as { success?: boolean; message?: string } | null;
        setActionMessage(outcome?.message ?? "Falha ao aplicar a ação.");
        if (outcome?.success) {
          if (actionName === "THROTTLE_DEFENDER_SCANS" || actionName === "RESTORE_DEFENDER_SCAN_SETTINGS") {
            const throttledAt = actionName === "THROTTLE_DEFENDER_SCANS" ? Date.now() : null;
            setDefenderThrottledAt(throttledAt);
            try {
              if (throttledAt) localStorage.setItem(DEFENDER_THROTTLE_STORAGE_KEY, String(throttledAt));
              else localStorage.removeItem(DEFENDER_THROTTLE_STORAGE_KEY);
            } catch {
              // Non-critical preference persistence.
            }
          }
          dismissInsight(insight);
        }
      } else {
        dismissInsight(insight);
      }
    } catch (e: any) {
      setActionMessage(e?.message ?? "Falha ao aplicar a ação.");
    } finally {
      setActionBusyKey(null);
    }
  };

  const applyLocally = async (insight: Insight) => {
    if (!insight.actionName) return;
    await applyAction(insight, insight.actionName, insight.actionContext, insightKey(insight));
  };

  const applySecondaryLocally = async (insight: Insight) => {
    if (!insight.secondaryActionName) return;
    await applyAction(insight, insight.secondaryActionName, insight.secondaryActionContext, `${insightKey(insight)}:secondary`);
  };

  const requestAgentApply = async (insight: Insight) => {
    if (!insight.actionName) return;
    // LOCAL_ONLY_ACTIONS never leave the machine - the action already runs
    // entirely through the local privileged helper (see system_repair.rs,
    // windows_actions::restart_pnp_device), so "let the agent do it" means
    // the local AnalystBlaze agent, not a server-queued RemoteCommand.
    // Delegate to the exact same call+outcome handling as "fazer eu mesmo" -
    // only the button that reached it differs.
    if (LOCAL_ONLY_ACTIONS.has(insight.actionName)) {
      track("insight_action_applied_by_local_agent", { actionName: insight.actionName });
      return applyLocally(insight);
    }
    const key = insightKey(insight);
    setActionBusyKey(key);
    setActionMessage(null);
    if (!onRequestAgentApplyInsight) return;
    try {
      await onRequestAgentApplyInsight(insight.actionName, insight.title, insight.explanation);
      track("insight_action_requested_from_agent", { actionName: insight.actionName });
      setActionMessage("Pedido enviado. O agente aplica na próxima sincronização (pode pedir confirmação local).");
      dismissInsight(insight);
    } catch (e: any) {
      setActionMessage(e?.message === "INSIGHT_ACTION_REQUIRES_PRO"
        ? "Deixar o agente aplicar sozinho e um recurso dos planos pagos. Você ainda pode fazer isso manualmente."
        : e?.message ?? "Falha ao pedir a ação ao agente.");
    } finally {
      setActionBusyKey(null);
    }
  };

  const diskUsageInsight = useMemo<Insight | null>(() => {
    const percent = telemetry?.disk_usage_percent;
    if (percent == null || !Number.isFinite(percent) || percent < DISK_USAGE_WARNING_THRESHOLD_PERCENT) {
      return null;
    }
    if (!onOpenDiskUsage) return null;
    return {
      title: "Disco quase cheio",
      explanation: `Seu disco está com ${Math.round(percent)}% de uso. Veja o detalhamento por jogos, apps, videos, downloads e arquivos grandes para decidir o que liberar.`,
      impact: `${Math.round(percent)}% usado`,
      category: "limpeza",
      action: {
        label: "Ver detalhes",
        onClick: () => {
          track("disk_usage_insight_opened", { percent: Math.round(percent) });
          onOpenDiskUsage();
        },
      },
    };
  }, [telemetry?.disk_usage_percent, onOpenDiskUsage, track]);

  const vpnLatencyInsight = useMemo<Insight | null>(() => {
    const diagnostics = networkDiagnostics;
    if (!diagnostics || !onOpenNetwork) return null;

    const recommendations = diagnostics.recommendations ?? [];
    if (!recommendations.includes("vpn_or_virtual_adapter_active")) return null;
    const activeLagFlags = NETWORK_LAG_FLAGS.filter((flag) => recommendations.includes(flag));
    if (activeLagFlags.length === 0) return null;

    // More simultaneous symptoms = more confident this isn't just noise.
    // A single metric barely over its threshold is weak evidence on its
    // own, so it gets dampened rather than trusted at face value - and if
    // that drops confidence below the bar, no insight is generated at all.
    let confidence = 0.4 + activeLagFlags.length * 0.15;
    if (activeLagFlags.length === 1 && activeLagFlags[0] === "latency_high") {
      const margin = (diagnostics.external_latency_ms ?? 0) - LATENCY_THRESHOLD_MS;
      if (margin < 20) confidence -= 0.15;
    }
    if (confidence < INSIGHT_CONFIDENCE_THRESHOLD) return null;

    const metricLabel: Record<(typeof NETWORK_LAG_FLAGS)[number], string> = {
      packet_loss_detected: `perda de pacotes de ${(diagnostics.packet_loss_percent ?? 0).toFixed(1)}% (limite: 2%)`,
      jitter_high: `jitter de ${Math.round(diagnostics.jitter_ms ?? 0)}ms (limite: 20ms)`,
      latency_high: `latência de ${Math.round(diagnostics.external_latency_ms ?? 0)}ms (limite: ${LATENCY_THRESHOLD_MS}ms)`,
    };
    const metricsText = activeLagFlags.map((flag) => metricLabel[flag]).join(", ");
    const adapterLabel = diagnostics.adapter_name || diagnostics.adapter_description || "adaptador de VPN/virtual";

    return {
      title: "VPN pode estar afetando sua conexão",
      explanation: `Detectamos ${metricsText}, com uma VPN ou adaptador virtual ativo (${adapterLabel}). A VPN e uma causa provavel, mas não confirmada - desative-a temporariamente para comparar a conexão antes de decidir.`,
      impact: activeLagFlags.length > 1 ? "Vários sinais de instabilidade" : "Instabilidade detectada",
      category: "rede",
      risk: "baixo",
      reversible: true,
      confidence,
      reason: metricsText,
      action: {
        label: "Ver detalhes em Rede",
        onClick: () => {
          track("vpn_latency_insight_opened", { confidence: Math.round(confidence * 100), flags: activeLagFlags.join(",") });
          onOpenNetwork();
        },
      },
    };
  }, [networkDiagnostics, onOpenNetwork, track]);

  // Explains the exact "AnalystBlaze says estável, but my game shows 500ms"
  // contradiction: external_latency_ms/jitter_ms above are always measured
  // against 1.1.1.1/8.8.8.8, which stay fast regardless of which game server
  // a match landed on - so a healthy general reading and a laggy match are
  // not actually in conflict, they are two different questions. Only fires
  // when the generic probes look genuinely healthy (no NETWORK_LAG_FLAGS) -
  // if those are already flagging trouble, vpnLatencyInsight above is the
  // more useful explanation and this would just be noise on top of it.
  const gameServerLatencyInsight = useMemo<Insight | null>(() => {
    const diagnostics = networkDiagnostics;
    const server = diagnostics?.game_server;
    if (!diagnostics || !server?.latency_ms || !onOpenNetwork) return null;

    const recommendations = diagnostics.recommendations ?? [];
    if (NETWORK_LAG_FLAGS.some((flag) => recommendations.includes(flag))) return null;

    const baseline = diagnostics.external_latency_ms;
    if (baseline == null) return null;
    const gap = server.latency_ms - baseline;
    if (gap < GAME_SERVER_LATENCY_GAP_MS || server.latency_ms < baseline * GAME_SERVER_LATENCY_RATIO) {
      return null;
    }

    const confidence = server.best_guess ? 0.55 : 0.7;
    const processLabel = server.process_name || "o jogo";
    const serverMs = Math.round(server.latency_ms);
    const baselineMs = Math.round(baseline);
    const certainty = server.best_guess
      ? "provavelmente o servidor do jogo, ou algo perto dele na rede da empresa"
      : "o servidor que o jogo está usando agora";

    return {
      title: "Sua internet está bem - o servidor do jogo que está longe ou sobrecarregado",
      explanation: `Sua conexão geral está saudavel: ${baselineMs}ms até a internet, sem perda de pacote nem instabilidade. Mas ${processLabel} está com ${serverMs}ms até ${certainty} (${server.remote_ip}) - bem mais que o normal. Isso normalmente significa que o servidor daquela partida está fisicamente longe, sobrecarregado, ou a rota específica até ele está ruim - nada disso e algo que o AnalystBlaze ou ajustes na sua rede conseguem corrigir, porque o problema começa do lado de fora da sua conexão.`,
      impact: `+${Math.round(gap)}ms só nesse servidor`,
      category: "rede",
      risk: "baixo",
      reversible: true,
      confidence,
      reason: `${serverMs}ms para ${server.remote_ip}:${server.remote_port} vs ${baselineMs}ms para a internet em geral`,
      action: {
        label: "Diagnosticar causa",
        onClick: () => {
          track("game_server_latency_insight_opened", {
            confidence: Math.round(confidence * 100),
            gapMs: Math.round(gap),
            bestGuess: server.best_guess,
          });
          onOpenNetwork(server.remote_ip);
        },
      },
    };
  }, [networkDiagnostics, onOpenNetwork, track]);

  const diskNearFullInsight = useMemo<Insight | null>(() => {
    const info = diskNearFullInfo;
    if (!info || !onOpenDiskUsage) return null;

    // A direct measurement (not an inference from correlated symptoms like
    // the VPN card above), so confidence starts high and just scales with
    // how far past the threshold it is.
    const confidence = Math.min(0.95, 0.7 + (info.usedPercent - DISK_NEAR_FULL_BASE_PERCENT) / 40);
    if (confidence < INSIGHT_CONFIDENCE_THRESHOLD) return null;

    const offendersText = info.topOffenders.length > 0
      ? info.topOffenders.map((item) => `${item.label} (${formatBytes(item.sizeBytes)})`).join(", ")
      : "não foi possível identificar os maiores itens agora";

    return {
      title: `Disco ${info.label || info.mountPoint} quase cheio`,
      explanation: `${info.mountPoint} está com ${Math.round(info.usedPercent)}% de uso. Maiores itens encontrados: ${offendersText}. Abra o Explorador de Disco para revisar e limpar com segurança (itens vao para quarentena reversível quando aplicavel).`,
      impact: `${Math.round(info.usedPercent)}% usado`,
      category: "limpeza",
      risk: "medio",
      reversible: true,
      confidence,
      reason: `Uso medido diretamente no volume ${info.mountPoint}: ${Math.round(info.usedPercent)}% (limite: ${DISK_NEAR_FULL_BASE_PERCENT}%).`,
      action: {
        label: "Ver detalhes",
        onClick: () => {
          track("disk_near_full_insight_opened", { mountPoint: info.mountPoint, usedPercent: Math.round(info.usedPercent) });
          onOpenDiskUsage();
        },
      },
    };
  }, [diskNearFullInfo, onOpenDiskUsage, track]);

  const scheduledDefragInsight = useMemo<Insight | null>(() => {
    const data = diskOptimizationInsightData;
    // Only ever fires for an HDD boot drive with the task off - an SSD
    // never gets this (it's TRIM'd, not defragmented) and an enabled task
    // is exactly the state that needs no attention.
    if (!data?.isHdd || data.defrag?.enabled !== false) return null;

    return {
      title: "Otimização automática de disco está desativada",
      explanation:
        "Seu disco principal é um HD (mecânico) e a desfragmentação agendada do Windows está desativada - isso e o que mantém a leitura de arquivos rápida com o tempo. Quer que eu reative?",
      impact: "HD sem otimização agendada",
      category: "limpeza",
      risk: "baixo",
      reversible: true,
      confidence: 0.9,
      reason: "Tarefa nativa \"Otimizar Unidades\" do Windows (ScheduledDefrag) está desativada.",
      actionName: "ENABLE_SCHEDULED_DEFRAG",
    };
  }, [diskOptimizationInsightData]);


  const modemUsbLinkIssue = useMemo<ModemUsbLinkIssue | null>(() => {
    const advanced = telemetry?.advanced as { modem_usb_link_issue?: ModemUsbLinkIssue | null } | null | undefined;
    return advanced?.modem_usb_link_issue ?? null;
  }, [telemetry?.advanced]);

  const [modemAnswer, setModemAnswer] = useState<ModemAnswer | null>(() => loadModemAnswer());
  const answerModemQuestion = (answer: ModemAnswer | null) => {
    track("modem_usb_link_answered", { answer: answer ?? "reset" });
    setModemAnswer(answer);
    try {
      if (answer) localStorage.setItem(MODEM_ANSWER_STORAGE_KEY, answer);
      else localStorage.removeItem(MODEM_ANSWER_STORAGE_KEY);
    } catch {
      // Non-critical preference persistence.
    }
  };

  // Two "device has a problem" cards Windows reports separately that are
  // really one fault - a USB-attached 4G/5G modem whose link keeps dropping
  // (see telemetry::modem_link for the correlation and the real case). The
  // generic per-device cards for those two are suppressed below in favour
  // of this one, since the disable/enable cycle they'd offer can't fix a
  // physical-layer problem - it was tried on the real machine and didn't.
  const modemUsbLinkInsight = useMemo<Insight | null>(() => {
    const issue = modemUsbLinkIssue;
    if (!issue) return null;

    const ghosts = issue.ghost_wwan_adapter_count;
    const port = issue.link.port != null ? `porta USB ${issue.link.port}` : "uma porta USB";
    const hub = issue.link.on_root_hub
      ? issue.link.hub_version
        ? `direto no controlador, hub raiz USB ${issue.link.hub_version}`
        : "direto no controlador"
      : "atras de um hub USB";
    const changeAnswer = { label: "Mudar resposta", onClick: () => answerModemQuestion(null) };

    if (modemAnswer === null) {
      return {
        title: "Você usa modem ou antena 4G/5G neste PC?",
        explanation: `Encontramos tres sinais que costumam ser um problema só: um adaptador de rede movel que não inicia, um dispositivo na ${port} que o Windows não consegue nem identificar, e ${ghosts} interfaces "Celular" antigas deixadas para tras. O diagnóstico certo depende de você ter ou não um modem 4G/5G ligado neste computador.`,
        impact: "Precisa de uma resposta sua",
        category: "rede",
        risk: "baixo",
        reversible: true,
        confidence: 0.8,
        reason: `${issue.usb_device_id}; ${issue.root_net_device_id ?? "sem ROOT\\NET"}; ${ghosts} interfaces WWAN ocultas`,
        action: { label: "Sim, uso modem/antena 4G ou 5G", onClick: () => answerModemQuestion("yes") },
        secondaryAction: { label: "Não uso", onClick: () => answerModemQuestion("no") },
      };
    }

    if (modemAnswer === "yes") {
      const isAmdUsb3 =
        issue.link.hub_version === "3.0" && (issue.link.controller ?? "").toUpperCase().includes("AMD");
      const amdNote = isAmdUsb3
        ? " - modem 4G/5G em porta USB 3.x de controlador AMD e uma incompatibilidade conhecida"
        : "";
      const hubStep = issue.link.on_root_hub
        ? "3) um hub USB com fonte própria entre o modem e o PC (modem 5G puxa mais corrente do que muita porta entrega)"
        : "3) ligar direto no PC em vez de no hub, ou trocar por um hub com fonte própria";
      return {
        title: "Sua antena 4G/5G está com a conexão USB instavel",
        explanation: `O Windows não consegue nem identificar o modem na ${port} (${hub}) - ele aparece como "dispositivo USB desconhecido", e por isso o adaptador de rede movel fica sem nada por tras e não inicia. As ${ghosts} interfaces "Celular" fantasmas registradas mostram que a conexão vive caindo e voltando. Isso e problema físico (porta, cabo ou energia), não de driver - por isso desativar/reativar não resolve. Testa nesta ordem: 1) uma porta USB 2.0 (geralmente as pretas) direto na traseira do gabinete${amdNote}; 2) sem cabo de extensao, ou com um mais curto e blindado; ${hubStep}; 4) desconecta, espera 15 segundos e reconecta.`,
        impact: `${ghosts} reconexoes registradas`,
        category: "rede",
        risk: "baixo",
        reversible: true,
        confidence: 0.85,
        reason: `${issue.usb_device_id} na ${port}, ${hub}${issue.link.controller ? ` (${issue.link.controller})` : ""}; ${issue.root_net_device_id ?? "sem ROOT\\NET"}; ${ghosts} interfaces WWAN ocultas`,
        secondaryAction: changeAnswer,
      };
    }

    return {
      title: "Adaptador de rede movel fantasma",
      explanation: `Existe um adaptador virtual de rede movel (Generic Mobile Broadband Adapter) sem nenhum modem real por tras, mais ${ghosts} interfaces "Celular" antigas. Como você não usa modem 4G/5G, isso e sobra de algum driver ou software antigo e pode ser removido pelo Gerenciador de Dispositivos (Exibir > Mostrar dispositivos ocultos > Adaptadores de rede). Já o dispositivo desconhecido na ${port} e outra coisa com conexão instavel - vale ver o que está ligado nela.`,
      impact: "Sobra de driver antigo",
      category: "rede",
      risk: "baixo",
      reversible: true,
      confidence: 0.6,
      reason: `${issue.root_net_device_id ?? "sem ROOT\\NET"}; ${ghosts} interfaces WWAN ocultas; ${issue.usb_device_id}`,
      secondaryAction: changeAnswer,
    };
    // answerModemQuestion is stable enough here (setState + localStorage); listing it
    // would only re-create the card on every render for no benefit.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [modemUsbLinkIssue, modemAnswer]);


  const [defenderThrottledAt, setDefenderThrottledAt] = useState<number | null>(() => loadDefenderThrottledAt());

  // "Windows Defender keeps reading the disk, it sits at 100%" - one
  // symptom, several faults. telemetry::disk_activity sees the pressure
  // (which process, which disk, for how long) and telemetry::defender_disk
  // names the cause from Defender's own log, its scan settings, NTFS/disk
  // errors and SMART. The card leads with the cause and quotes the numbers
  // it was decided on, and only offers to slow Defender down in the one
  // case where that is the actual fix - for a failing disk or a corrupt
  // file system, throttling the scan would just hide the real problem.
  const defenderDiskInsight = useMemo<Insight | null>(() => {
    const advanced = telemetry?.advanced as { defender_disk_issue?: DefenderDiskIssue | null } | null | undefined;
    const issue = advanced?.defender_disk_issue ?? null;
    if (!issue) return null;
    const live = telemetry?.disk_activity ?? null;

    const diskLetter = firstDriveLetter(issue.disk);
    const diskLabel = diskLetter ? `disco ${diskLetter}:` : issue.disk_number != null ? `disco ${issue.disk_number}` : "disco";
    const activePercent = Math.round(live?.busiest_disk_active_percent ?? issue.peak_disk_active_percent);
    const share = live?.defender ? Math.round(live.defender.share_percent) : null;
    const minutes = Math.max(issue.sustained_minutes, Math.round((live?.pressure.sustained_seconds ?? 0) / 60));
    const pressureLine = `O Windows Defender (MsMpEng) vem mantendo o ${diskLabel} em ~${activePercent}% de atividade por ${minutes} min nos últimos 30 minutos${share != null ? `, respondendo por ~${share}% de tudo que esta sendo lido/gravado` : ""}.`;
    const reasonBase = `${issue.disk ?? "disco ?"} ${Math.round(issue.peak_disk_active_percent)}% ativo; Defender ${issue.peak_defender_mb_s.toFixed(1)} MB/s pico; ${issue.sustained_minutes} min sob pressão`;

    switch (issue.cause) {
      case "disk_failing": {
        const evidence =
          issue.disk_error_events_7d > 0
            ? `${issue.disk_error_events_7d} erros de hardware registrados pelo Windows nesse disco nos últimos 7 dias`
            : "o próprio disco (SMART) está avisando que vai falhar";
        return {
          title: `O ${diskLabel} está com defeito - o Defender e só a vitima`,
          explanation: `${pressureLine} Mas a causa não e o antivirus: ${evidence}. Um disco falhando responde devagar a qualquer leitura, e o Defender só e o programa que mais le. Limitar o Defender esconderia o aviso. O que fazer: faça backup do que importa agora, enquanto o disco ainda responde, e planeje a troca. Depois do backup, o disco pode ser visto em Telemetria > Saúde do disco.`,
          impact: "Risco de perda de dados",
          category: "performance",
          risk: "alto",
          reversible: false,
          confidence: 0.75,
          reason: `${reasonBase}; ${issue.disk_error_events_7d} eventos disk/storahci em 7d; SMART predict_failure`,
        };
      }
      case "file_system_corruption": {
        const drive = issue.unhealthy_volumes[0] ?? diskLetter ?? "C";
        const evidence = [
          issue.unhealthy_volumes.length > 0 ? `o Windows marca a unidade ${issue.unhealthy_volumes.join(", ")} como não saudavel` : null,
          issue.ntfs_corruption_events_7d > 0 ? `${issue.ntfs_corruption_events_7d} evento(s) de corrupcao NTFS nos últimos 7 dias` : null,
        ]
          .filter(Boolean)
          .join(" e ");
        return {
          title: `Sistema de arquivos da unidade ${drive}: corrompido`,
          explanation: `${pressureLine} A causa está no disco, não no antivirus: ${evidence}. Estruturas corrompidas fazem cada leitura travar e serem tentadas de novo - e o Defender, que le tudo, e quem aparece no Gerenciador de Tarefas. A correção e o chkdsk do próprio Windows, que precisa rodar antes do sistema abrir: agende aqui e reinicie quando puder.`,
          impact: "Leituras travando em arquivos corrompidos",
          category: "performance",
          risk: "baixo",
          reversible: true,
          confidence: 0.8,
          reason: `${reasonBase}; volumes não saudaveis: ${issue.unhealthy_volumes.join(", ") || "nenhum"}; Ntfs 55/130: ${issue.ntfs_corruption_events_7d}`,
          actionName: "SCHEDULE_VOLUME_CHECK",
          actionContext: { driveLetter: drive },
          actionLabel: `Verificar ${drive}: na próxima reinicialização`,
        };
      }
      case "threat_remediation_loop":
        return {
          title: "O Defender está encontrando a mesma ameaca repetidamente",
          explanation: `${pressureLine} O histórico do próprio Defender mostra ${issue.detections_24h} detecção(oes) nas últimas 24h${issue.remediation_failures_24h > 0 ? ` e ${issue.remediation_failures_24h} tentativa(s) de remocao que falharam` : ""}. Quando ele não consegue remover ou colocar em quarentena, encontra de novo na próxima verificação, e assim por diante - por isso o disco não descansa. Abra o Histórico de proteção do Windows, veja qual arquivo e, e escolha remover ou permitir. O AnalystBlaze não decide isso por você: e a diferenca entre um falso positivo num jogo e um malware de verdade.`,
          impact: `${issue.detections_24h} detecções em 24h`,
          category: "performance",
          risk: "baixo",
          reversible: true,
          confidence: 0.8,
          reason: `${reasonBase}; Defender 1116: ${issue.detections_24h}; 1008/1118/1119: ${issue.remediation_failures_24h}`,
          action: {
            label: "Abrir histórico de proteção",
            onClick: () => {
              track("defender_disk_open_history");
              void openWindowsSecurity("history").catch(() => undefined);
            },
          },
        };
      case "scan_cannot_finish":
        return {
          title: "As verificações do Defender não estão conseguindo terminar",
          explanation: `${pressureLine} Nas últimas 24h o Defender iniciou ${issue.scans_started_24h} verificação(oes), concluiu ${issue.scans_finished_24h}${issue.scans_failed_24h > 0 ? ` e registrou ${issue.scans_failed_24h} como falha` : ""}. Isso normalmente e um arquivo que ele não consegue atravessar - um arquivo enorme ou corrompido, um container (VHD, PST, arquivo de backup) ou uma pasta de nuvem só com placeholders - e que e tentado de novo a cada verificação. Renovar as definições resolve os casos em que o erro está no próprio Defender; se continuar, o Histórico de proteção mostra em qual pasta ele está parando, e dai vale excluir aquela pasta específica da verificação (em Segurança do Windows), nunca o disco inteiro.`,
          impact: `${issue.scans_started_24h} verificações, ${issue.scans_finished_24h} concluidas`,
          category: "performance",
          risk: "baixo",
          reversible: true,
          confidence: 0.7,
          reason: `${reasonBase}; Defender 1000/1001/1005: ${issue.scans_started_24h}/${issue.scans_finished_24h}/${issue.scans_failed_24h}`,
          actionName: "RENEW_DEFENDER_DEFINITIONS",
          actionLabel: "Renovar definições do Defender",
          action: {
            label: "Abrir histórico de proteção",
            onClick: () => {
              track("defender_disk_open_history");
              void openWindowsSecurity("history").catch(() => undefined);
            },
          },
        };
      case "definitions_or_engine_failing":
        return {
          title: "As definições ou o motor do Defender estão falhando",
          explanation: `${pressureLine} O log do Defender registrou ${issue.signature_update_failures_24h} falha(s) ao atualizar as definições${issue.engine_failures_24h > 0 ? ` e ${issue.engine_failures_24h} erro(s) do proprio motor de verificacao` : ""} nas últimas 24h. Um conjunto de definições corrompido e uma causa classica de verificação em loop. A correção e remover as definições atuais e baixar de novo - o Defender fica alguns minutos sem definições até o download terminar.`,
          impact: `${issue.signature_update_failures_24h} falhas de atualização em 24h`,
          category: "performance",
          risk: "baixo",
          reversible: true,
          confidence: 0.7,
          reason: `${reasonBase}; Defender 2001: ${issue.signature_update_failures_24h}; 3002/5008: ${issue.engine_failures_24h}`,
          actionName: "RENEW_DEFENDER_DEFINITIONS",
          actionLabel: "Renovar definições do Defender",
        };
      case "unthrottled_scan": {
        const factor = issue.scan_avg_cpu_load_factor ?? 50;
        const settings = [
          `pode usar até ${factor}% da CPU`,
          issue.disable_cpu_throttle_on_idle_scans ? "ignora o limite quando o PC está ocioso" : null,
          issue.scan_only_if_idle === false ? "roda mesmo com você usando o PC" : null,
        ]
          .filter(Boolean)
          .join(", ");
        return {
          title: "Uma verificação do Defender está saturando o disco",
          explanation: `${pressureLine} Não encontramos nenhum sinal de defeito no disco, corrupcao ou erro do Defender - e uma verificação normal, só que com as configurações padrão do Windows (${settings}) num disco que não acompanha o ritmo. Limitar a verificação a 20% de CPU e só com o PC ocioso deixa ela mais lenta, mas para de travar todo o resto. A proteção em tempo real não muda.${issue.full_scan_age_days == null ? " Nenhuma verificacao completa consta - a primeira e sempre a mais pesada." : ""}`,
          impact: `${diskLabel} em ~${activePercent}%`,
          category: "performance",
          risk: "baixo",
          reversible: true,
          confidence: 0.7,
          reason: `${reasonBase}; ScanAvgCPULoadFactor=${factor}; DisableCpuThrottleOnIdleScans=${String(issue.disable_cpu_throttle_on_idle_scans)}; ScanOnlyIfIdleEnabled=${String(issue.scan_only_if_idle)}`,
          actionName: "THROTTLE_DEFENDER_SCANS",
          actionLabel: "Limitar verificações do Defender",
        };
      }
      default:
        return {
          title: "O Defender está saturando o disco, sem causa evidente",
          explanation: `${pressureLine} As verificações já estão com limite de CPU, e nem o log do Defender, nem o disco, nem o sistema de arquivos apontam um motivo. Vale olhar o Histórico de proteção (pode haver uma detecção antiga em loop que o log não mostra mais) e, se acontecer todo dia no mesmo horário, e a verificação agendada - mude o horário em Segurança do Windows. Se persistir, renovar as definições e um passo seguro.`,
          impact: `${diskLabel} em ~${activePercent}%`,
          category: "performance",
          risk: "baixo",
          reversible: true,
          confidence: 0.5,
          reason: reasonBase,
          actionName: "RENEW_DEFENDER_DEFINITIONS",
          actionLabel: "Renovar definições do Defender",
          action: {
            label: "Abrir histórico de proteção",
            onClick: () => {
              track("defender_disk_open_history");
              void openWindowsSecurity("history").catch(() => undefined);
            },
          },
        };
    }
  }, [telemetry, track]);

  const defenderThrottledInsight = useMemo<Insight | null>(() => {
    if (defenderThrottledAt == null) return null;
    return {
      title: "Verificações do Defender limitadas pelo AnalystBlaze",
      explanation: `Desde ${new Date(defenderThrottledAt).toLocaleString()} as verificações agendadas do Defender usam no máximo 20% da CPU e só rodam com o PC ocioso. Se quiser voltar ao padrão do Windows, restaure aqui - os valores anteriores exatos foram guardados.`,
      impact: "Verificações mais lentas, PC mais livre",
      category: "performance",
      risk: "baixo",
      reversible: true,
      confidence: 1,
      reason: "THROTTLE_DEFENDER_SCANS aplicado localmente",
      actionName: "RESTORE_DEFENDER_SCAN_SETTINGS",
      actionLabel: "Restaurar padrão do Windows",
    };
  }, [defenderThrottledAt]);


  const visibleInsights = useMemo(() => {
    // Reinício pendente, pontos de restauração (Shadow Storage), serviços e
    // eventos do Windows e dispositivos com erro moraram aqui; agora ficam em
    // Meu PC › Saúde do Windows (doc 14 §1.3), um lugar só.
    const local = [
      diskUsageInsight,
      vpnLatencyInsight,
      gameServerLatencyInsight,
      diskNearFullInsight,
      scheduledDefragInsight,
      defenderDiskInsight,
      defenderThrottledInsight,
      modemUsbLinkInsight,
    ].filter((insight): insight is Insight => insight != null);
    const all = [...local, ...insights];
    return all.filter((insight) => !(insightKey(insight) in dismissed));
  }, [
    diskUsageInsight,
    vpnLatencyInsight,
    gameServerLatencyInsight,
    diskNearFullInsight,
    scheduledDefragInsight,
    defenderDiskInsight,
    defenderThrottledInsight,
    modemUsbLinkInsight,
    insights,
    dismissed,
  ]);

  const generate = async () => {
    setLoading(true);
    setError(null);
    if (isTauriRuntime()) {
      // Best-effort, independent of the server fetch below - a failure here
      // just means the local VPN+latency insight stays silent, not that the
      // whole insights screen errors out.
      networkDiagnosticsResource.refresh().catch(() => undefined);
      checkDiskOptimizationInsight()
        .then(setDiskOptimizationInsightData)
        .catch(() => undefined);
    }
    try {
      const result = await fetchInsights(t, locale);
      setInsights(result.insights);
      track("insights_refreshed", { source: result.source });
    } catch (e: any) {
      setError(e?.message ?? t("insights.errorFallback"));
      track("insights_refresh_failed");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    generate();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [locale]);

  const openOnWeb = async () => {
    try {
      const url = await openAgentInsights();
      if (!isTauriRuntime()) {
        window.open(url, "_blank", "noopener,noreferrer");
      }
      track("insights_opened_on_web");
    } catch (e: any) {
      setError(e?.message ?? t("insights.errorFallback"));
    }
  };

  const healthWarnings = windowsHealthItems({ telemetry: telemetry ?? null, shadowConsentNeeded: Boolean(shadowConsentNeeded) }).filter(
    (item) => item.severity !== "ok",
  );
  const shown = showAll ? visibleInsights : visibleInsights.slice(0, MAX_VISIBLE);
  const hiddenCount = visibleInsights.length - shown.length;
  const silencedCount = Object.keys(dismissed).length;

  const riskLine = (ins: Insight) =>
    [
      ins.risk ? t(`insightsD3.risk.${ins.risk}`) : null,
      ins.reversible === true ? t("insightsD3.reversible") : ins.reversible === false ? t("insightsD3.notReversible") : null,
      ins.impact || null,
    ]
      .filter(Boolean)
      .join(" · ");

  return (
    <div className="grid grid-cols-[minmax(0,1.6fr)_minmax(0,1fr)] gap-5">
      <div className="flex flex-col gap-5">
        <header className="flex flex-col gap-3">
          <div>
            <h2 className="text-[28px] font-semibold tracking-tight text-fg">
              {visibleInsights.length > 0
                ? t("insightsD3.title", { count: Math.min(visibleInsights.length, MAX_VISIBLE) })
                : t("insightsD3.titleNone")}
            </h2>
            <p className="mt-1 text-sm text-fg-3">{t("insightsD3.subtitle")}</p>
          </div>
          <div className="flex gap-2">
            <Button size="sm" variant="outline" onClick={() => void generate()} disabled={loading}>
              <RefreshCw className={loading ? "animate-spin" : undefined} />
              {loading ? t("common.refreshing") : t("common.refresh")}
            </Button>
            <Button size="sm" variant="link" onClick={() => void openOnWeb()}>
              <ExternalLink />
              {t("insights.openInWeb")}
            </Button>
          </div>
        </header>

        {actionMessage && <p className="rounded-control border border-line bg-surface-2 px-4 py-3 text-sm text-fg-2">{actionMessage}</p>}
        {error && visibleInsights.length > 0 && <p className="text-sm text-danger">{error}</p>}

        {loading && visibleInsights.length === 0 ? (
          <EmptyState title={t("insightsD3.loading")} />
        ) : error && visibleInsights.length === 0 ? (
          <EmptyState
            icon={<AlertCircle />}
            title={t("insights.errorTitle")}
            reason={error}
            action={
              <Button variant="outline" onClick={() => void generate()}>
                <RefreshCw />
                {t("insights.errorRetry")}
              </Button>
            }
          />
        ) : visibleInsights.length === 0 ? (
          <EmptyState title={t("insightsD3.emptyTitle")} reason={t("insightsD3.emptyReason")} />
        ) : (
          <>
            {shown.map((ins, index) => {
              const key = insightKey(ins);
              const busy = actionBusyKey === key;
              const canRunLocally = Boolean(
                ins.actionName && LOCALLY_EXECUTABLE_ACTIONS.has(ins.actionName) && onApplyInsightActionLocally,
              );
              const canRequestAgent = Boolean(
                ins.actionName &&
                  (LOCAL_ONLY_ACTIONS.has(ins.actionName) ? onApplyInsightActionLocally : onRequestAgentApplyInsight),
              );
              const secondaryBusy = actionBusyKey === `${key}:secondary`;
              const canRunSecondaryLocally = Boolean(
                ins.secondaryActionName && LOCALLY_EXECUTABLE_ACTIONS.has(ins.secondaryActionName) && onApplyInsightActionLocally,
              );
              // Um primário por tela: só a sugestão mais importante (a primeira).
              const mainVariant = index === 0 ? "primary" : "outline";
              const line = riskLine(ins);
              return (
                <Card key={key} padding="lg">
                  <h3 className="text-[18px] font-semibold tracking-tight text-fg">{ins.title}</h3>
                  <p className="mt-2 text-sm leading-relaxed text-fg-2">{ins.explanation}</p>
                  {line && <p className="mt-2 text-xs text-fg-3">{line}</p>}
                  <div className="mt-4 flex flex-wrap items-center gap-x-4 gap-y-2">
                    {canRequestAgent ? (
                      <Button variant={mainVariant} disabled={busy} onClick={() => void requestAgentApply(ins)}>
                        {busy ? t("controls.d3.working") : t("insightsD3.letDo")}
                      </Button>
                    ) : canRunLocally ? (
                      <Button variant={mainVariant} disabled={busy} onClick={() => void applyLocally(ins)}>
                        {busy ? t("controls.d3.working") : ins.actionLabel ?? t("insightsD3.doNow")}
                      </Button>
                    ) : ins.action ? (
                      <Button variant={mainVariant} onClick={ins.action.onClick}>
                        {ins.action.label}
                      </Button>
                    ) : null}
                    {canRequestAgent && canRunLocally && (
                      <Button variant="link" disabled={busy} onClick={() => void applyLocally(ins)}>
                        {ins.actionLabel ?? t("insightsD3.doMyself")}
                      </Button>
                    )}
                    {ins.action && (canRequestAgent || canRunLocally) && (
                      <Button variant="link" onClick={ins.action.onClick}>
                        {ins.action.label}
                      </Button>
                    )}
                    {ins.secondaryAction && (
                      <Button variant="link" onClick={ins.secondaryAction.onClick}>
                        {ins.secondaryAction.label}
                      </Button>
                    )}
                    {canRunSecondaryLocally && (
                      <Button size="sm" variant="outline" disabled={secondaryBusy} onClick={() => void applySecondaryLocally(ins)}>
                        {ins.secondaryActionLabel ?? t("insightsD3.disable")}
                      </Button>
                    )}
                    <button
                      type="button"
                      onClick={() => dismissInsight(ins)}
                      className="ml-auto text-sm text-fg-3 transition-colors hover:text-fg"
                    >
                      {t("insightsD3.notNow")}
                    </button>
                  </div>
                </Card>
              );
            })}
            {(hiddenCount > 0 || showAll) && visibleInsights.length > MAX_VISIBLE && (
              <Button variant="link" className="self-start" onClick={() => setShowAll((value) => !value)}>
                {showAll ? t("insightsD3.showLess") : t("insightsD3.showAll", { count: hiddenCount })}
              </Button>
            )}
          </>
        )}
      </div>

      <aside className="flex flex-col gap-5">
        <Card
          label={
            healthWarnings.length > 0
              ? t("insightsD3.healthLabelCount", { count: healthWarnings.length })
              : t("insightsD3.healthLabel")
          }
          className={healthWarnings.length > 0 ? "border-warn/40" : undefined}
        >
          <p className="text-sm text-fg-2">
            {healthWarnings.length > 0
              ? healthWarnings.map((item) => t(`mypc.health.items.${item.titleKey}`, item.titleParams)).join(" · ")
              : t("insightsD3.healthOk")}
          </p>
          <p className="mt-1 text-xs text-fg-3">{t("insightsD3.healthNote")}</p>
          {onOpenWindowsHealth && (
            <Button size="sm" variant="link" className="mt-3" onClick={onOpenWindowsHealth}>
              {t("insightsD3.healthOpen")}
              <ArrowRight />
            </Button>
          )}
        </Card>

        <Card label={t("insightsD3.howLabel")}>
          <ul className="flex list-disc flex-col gap-1.5 pl-4 text-sm text-fg-2">
            <li>{t("insightsD3.how.max")}</li>
            <li>{t("insightsD3.how.notNow")}</li>
            <li>{t("insightsD3.how.nothingAlone")}</li>
            <li>{t("insightsD3.how.history")}</li>
          </ul>
        </Card>

        {silencedCount > 0 && (
          <Button
            variant="link"
            className="self-start"
            onClick={() => {
              setDismissed({});
              try {
                localStorage.removeItem(DISMISS_STORAGE_KEY);
              } catch {
                // Non-critical preference persistence.
              }
            }}
          >
            {t("insightsD3.silenced", { count: silencedCount })}
          </Button>
        )}
      </aside>
    </div>
  );
}

function categoryLabel(category: Category, t: (key: string) => string) {
  const keys: Record<Category, string> = {
    performance: "insights.categories.performance",
    energia: "insights.categories.energy",
    rede: "insights.categories.network",
    limpeza: "insights.categories.cleanup",
  };
  return t(keys[category] ?? keys.performance);
}
