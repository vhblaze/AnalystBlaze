import { useEffect, useMemo, useState } from "react";
import { AlertCircle, ArrowRight, Bot, Brain, Cpu, Droplets, ExternalLink, RefreshCw, Sparkles, User, Wind, X, Zap } from "lucide-react";
import { fetchInsights, type DiskNearFullInfo, type Insight } from "@/services/insights";
import { useI18n } from "@/i18n";
import { useTelemetry } from "@/hooks/useTelemetry";
import {
  checkDiskOptimizationInsight,
  getNetworkDiagnostics,
  isTauriRuntime,
  openAgentInsights,
  type AgentTelemetrySnapshot,
  type DiskOptimizationInsight,
  type NetworkDiagnostics,
} from "@/services/tauri/agent";

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

function formatBytesShort(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value >= 100 || unitIndex === 0 ? Math.round(value) : value.toFixed(1)} ${units[unitIndex]}`;
}
const DISMISS_STORAGE_KEY = "analystblaze.dismissedInsights";
const DISMISS_TTL_MS = 24 * 60 * 60 * 1000;
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
]);
/** Actions whose useAuth wrapper resolves with the real outcome instead of
 * throwing on failure (see restartPnpDevice's comment in useAuth.ts) -
 * "didn't fix it" is legitimate information to show, not an exception, so
 * these only dismiss the card when the outcome actually says success. */
const HONEST_OUTCOME_ACTIONS = new Set(["RESTART_PNP_DEVICE", "REPAIR_SERVICE", "DISABLE_SERVICE_PERMANENTLY"]);
/** How many Critical-level (level 1) Windows Event Log entries in the last
 * 24h are worth mentioning. This is deliberately conservative and paired
 * with "pode (nao necessariamente) indicar" wording below, not "seu Windows
 * esta com problema" - a healthy PC can log a handful of criticals from
 * completely unrelated one-off causes, so this is a prompt to check, not a
 * diagnosis. */
const EVENT_LOG_CRITICAL_THRESHOLD = 10;
// Human labels for the Win32_PnPEntity ConfigManagerErrorCode values the
// backend already filters to (see telemetry::advanced::PROBLEM_DEVICE_CODES) -
// matches the Device Manager wording users may already be familiar with.
const PROBLEM_CODE_LABELS: Record<number, string> = {
  10: "Codigo 10 - o dispositivo nao consegue iniciar",
  12: "Codigo 12 - recursos de hardware insuficientes",
  14: "Codigo 14 - precisa reiniciar o Windows para funcionar",
  18: "Codigo 18 - drivers precisam ser reinstalados",
  19: "Codigo 19 - configuracao do Registro corrompida",
  28: "Codigo 28 - drivers nao instalados",
  31: "Codigo 31 - o Windows nao conseguiu iniciar o dispositivo",
  37: "Codigo 37 - o driver retornou um erro",
  38: "Codigo 38 - uma instancia anterior do driver ainda esta carregada",
  39: "Codigo 39 - driver corrompido ou ausente",
  40: "Codigo 40 - entrada do Registro do driver corrompida",
  43: "Codigo 43 - o Windows parou o dispositivo por um problema reportado",
  48: "Codigo 48 - driver bloqueado por incompatibilidade",
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
    tone: "from-cyan-500/15 to-cyan-500/0",
    ring: "border-cyan-400/25",
    chip: "border-cyan-400/30 bg-cyan-500/10 text-cyan-200",
  },
  energia: {
    icon: Cpu,
    tone: "from-amber-500/15 to-amber-500/0",
    ring: "border-amber-400/25",
    chip: "border-amber-400/30 bg-amber-500/10 text-amber-200",
  },
  rede: {
    icon: Wind,
    tone: "from-violet-500/15 to-violet-500/0",
    ring: "border-violet-400/25",
    chip: "border-violet-400/30 bg-violet-500/10 text-violet-200",
  },
  limpeza: {
    icon: Droplets,
    tone: "from-emerald-500/15 to-emerald-500/0",
    ring: "border-emerald-400/25",
    chip: "border-emerald-400/30 bg-emerald-500/10 text-emerald-200",
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
  onResolveShadowConsent,
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
}) {
  const { t, locale } = useI18n();
  const track = useTelemetry("insights");
  const [insights, setInsights] = useState<Insight[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState<Record<string, number>>(() => loadDismissed());
  const [actionBusyKey, setActionBusyKey] = useState<string | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  // Only fetched here, on demand, for the local VPN+latency insight below -
  // the passive telemetry sample doesn't probe the adapter/VPN detection
  // (kept out of the 2s telemetry loop deliberately, see collect_network_sample).
  const [networkDiagnostics, setNetworkDiagnostics] = useState<NetworkDiagnostics | null>(null);
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
        setActionMessage("Verificacao iniciada em segundo plano. Acompanhe o progresso em Controles > Avancado > Saude do Windows.");
        dismissInsight(insight);
      } else if (HONEST_OUTCOME_ACTIONS.has(actionName)) {
        const outcome = result as { success?: boolean; message?: string } | null;
        setActionMessage(outcome?.message ?? "Falha ao aplicar a acao.");
        if (outcome?.success) dismissInsight(insight);
      } else {
        dismissInsight(insight);
      }
    } catch (e: any) {
      setActionMessage(e?.message ?? "Falha ao aplicar a acao.");
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
      setActionMessage("Pedido enviado. O agente aplica na proxima sincronizacao (pode pedir confirmacao local).");
      dismissInsight(insight);
    } catch (e: any) {
      setActionMessage(e?.message === "INSIGHT_ACTION_REQUIRES_PRO"
        ? "Deixar o agente aplicar sozinho e um recurso dos planos pagos. Voce ainda pode fazer isso manualmente."
        : e?.message ?? "Falha ao pedir a acao ao agente.");
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
      explanation: `Seu disco esta com ${Math.round(percent)}% de uso. Veja o detalhamento por jogos, apps, videos, downloads e arquivos grandes para decidir o que liberar.`,
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
      latency_high: `latencia de ${Math.round(diagnostics.external_latency_ms ?? 0)}ms (limite: ${LATENCY_THRESHOLD_MS}ms)`,
    };
    const metricsText = activeLagFlags.map((flag) => metricLabel[flag]).join(", ");
    const adapterLabel = diagnostics.adapter_name || diagnostics.adapter_description || "adaptador de VPN/virtual";

    return {
      title: "VPN pode estar afetando sua conexao",
      explanation: `Detectamos ${metricsText}, com uma VPN ou adaptador virtual ativo (${adapterLabel}). A VPN e uma causa provavel, mas nao confirmada - desative-a temporariamente para comparar a conexao antes de decidir.`,
      impact: activeLagFlags.length > 1 ? "Varios sinais de instabilidade" : "Instabilidade detectada",
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

  // Explains the exact "AnalystBlaze says estavel, but my game shows 500ms"
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
      : "o servidor que o jogo esta usando agora";

    return {
      title: "Sua internet esta bem - o servidor do jogo que esta longe ou sobrecarregado",
      explanation: `Sua conexao geral esta saudavel: ${baselineMs}ms ate a internet, sem perda de pacote nem instabilidade. Mas ${processLabel} esta com ${serverMs}ms ate ${certainty} (${server.remote_ip}) - bem mais que o normal. Isso normalmente significa que o servidor daquela partida esta fisicamente longe, sobrecarregado, ou a rota especifica ate ele esta ruim - nada disso e algo que o AnalystBlaze ou ajustes na sua rede conseguem corrigir, porque o problema comeca do lado de fora da sua conexao.`,
      impact: `+${Math.round(gap)}ms so nesse servidor`,
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
      ? info.topOffenders.map((item) => `${item.label} (${formatBytesShort(item.sizeBytes)})`).join(", ")
      : "nao foi possivel identificar os maiores itens agora";

    return {
      title: `Disco ${info.label || info.mountPoint} quase cheio`,
      explanation: `${info.mountPoint} esta com ${Math.round(info.usedPercent)}% de uso. Maiores itens encontrados: ${offendersText}. Abra o Explorador de Disco para revisar e limpar com seguranca (itens vao para quarentena reversivel quando aplicavel).`,
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
      title: "Otimizacao automatica de disco esta desativada",
      explanation:
        "Seu disco principal e um HD (mecanico) e a desfragmentacao agendada do Windows esta desativada - isso e o que mantem a leitura de arquivos rapida com o tempo. Quer que eu reative?",
      impact: "HD sem otimizacao agendada",
      category: "limpeza",
      risk: "baixo",
      reversible: true,
      confidence: 0.9,
      reason: "Tarefa nativa \"Otimizar Unidades\" do Windows (ScheduledDefrag) esta desativada.",
      actionName: "ENABLE_SCHEDULED_DEFRAG",
    };
  }, [diskOptimizationInsightData]);

  const systemHealthInsight = useMemo<Insight | null>(() => {
    const advanced = telemetry?.advanced as
      | {
          event_log_critical_errors_24h?: number | null;
          failing_services?: Array<{ name: string; kind: string }>;
          shell_crashes?: Array<{ process: string }>;
        }
      | null
      | undefined;
    if (!advanced) return null;

    const criticalErrors = advanced.event_log_critical_errors_24h ?? 0;
    const failingServices = advanced.failing_services ?? [];
    const shellCrashes = advanced.shell_crashes ?? [];
    if (criticalErrors < EVENT_LOG_CRITICAL_THRESHOLD && failingServices.length === 0) {
      return null;
    }

    const signals: string[] = [];
    if (criticalErrors >= EVENT_LOG_CRITICAL_THRESHOLD) {
      signals.push(`${criticalErrors} erros criticos no Visualizador de Eventos nas ultimas 24h`);
    }
    if (failingServices.length > 0) {
      const names = failingServices.slice(0, 3).map((service) => service.name).join(", ");
      signals.push(`${failingServices.length} servico(s) do Windows falhando (${names})`);
    }
    if (shellCrashes.length > 0) {
      signals.push(`${shellCrashes.length} travamento(s) recente(s) do shell do Windows`);
    }

    // More independent signals firing together raises confidence a bit -
    // still capped well below "certain", since none of this confirms actual
    // file corruption, only correlates with the kind of problem sfc/DISM
    // can rule in or out.
    const confidence = Math.min(0.65, 0.35 + signals.length * 0.1);

    return {
      title: "Possivel problema no Windows",
      explanation: `Detectamos ${signals.join(" e ")}. Isso pode (nao necessariamente) indicar arquivos de sistema corrompidos. Uma verificacao com as ferramentas oficiais do Windows (sfc, e DISM se precisar) confirma e corrige - leva alguns minutos rodando em segundo plano.`,
      impact: "Possivel corrupcao de arquivos de sistema",
      category: "performance",
      risk: "baixo",
      reversible: true,
      confidence,
      reason: signals.join("; "),
      actionName: "START_SYSTEM_FILE_CHECK",
    };
  }, [telemetry?.advanced]);

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
        title: "Voce usa modem ou antena 4G/5G neste PC?",
        explanation: `Encontramos tres sinais que costumam ser um problema so: um adaptador de rede movel que nao inicia, um dispositivo na ${port} que o Windows nao consegue nem identificar, e ${ghosts} interfaces "Celular" antigas deixadas para tras. O diagnostico certo depende de voce ter ou nao um modem 4G/5G ligado neste computador.`,
        impact: "Precisa de uma resposta sua",
        category: "rede",
        risk: "baixo",
        reversible: true,
        confidence: 0.8,
        reason: `${issue.usb_device_id}; ${issue.root_net_device_id ?? "sem ROOT\\NET"}; ${ghosts} interfaces WWAN ocultas`,
        action: { label: "Sim, uso modem/antena 4G ou 5G", onClick: () => answerModemQuestion("yes") },
        secondaryAction: { label: "Nao uso", onClick: () => answerModemQuestion("no") },
      };
    }

    if (modemAnswer === "yes") {
      const isAmdUsb3 =
        issue.link.hub_version === "3.0" && (issue.link.controller ?? "").toUpperCase().includes("AMD");
      const amdNote = isAmdUsb3
        ? " - modem 4G/5G em porta USB 3.x de controlador AMD e uma incompatibilidade conhecida"
        : "";
      const hubStep = issue.link.on_root_hub
        ? "3) um hub USB com fonte propria entre o modem e o PC (modem 5G puxa mais corrente do que muita porta entrega)"
        : "3) ligar direto no PC em vez de no hub, ou trocar por um hub com fonte propria";
      return {
        title: "Sua antena 4G/5G esta com a conexao USB instavel",
        explanation: `O Windows nao consegue nem identificar o modem na ${port} (${hub}) - ele aparece como "dispositivo USB desconhecido", e por isso o adaptador de rede movel fica sem nada por tras e nao inicia. As ${ghosts} interfaces "Celular" fantasmas registradas mostram que a conexao vive caindo e voltando. Isso e problema fisico (porta, cabo ou energia), nao de driver - por isso desativar/reativar nao resolve. Testa nesta ordem: 1) uma porta USB 2.0 (geralmente as pretas) direto na traseira do gabinete${amdNote}; 2) sem cabo de extensao, ou com um mais curto e blindado; ${hubStep}; 4) desconecta, espera 15 segundos e reconecta.`,
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
      explanation: `Existe um adaptador virtual de rede movel (Generic Mobile Broadband Adapter) sem nenhum modem real por tras, mais ${ghosts} interfaces "Celular" antigas. Como voce nao usa modem 4G/5G, isso e sobra de algum driver ou software antigo e pode ser removido pelo Gerenciador de Dispositivos (Exibir > Mostrar dispositivos ocultos > Adaptadores de rede). Ja o dispositivo desconhecido na ${port} e outra coisa com conexao instavel - vale ver o que esta ligado nela.`,
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

  const failingDeviceInsights = useMemo<Insight[]>(() => {
    const advanced = telemetry?.advanced as
      | { failing_devices?: Array<{ name?: string | null; device_id: string; device_class?: string | null; problem_code: number }> }
      | null
      | undefined;
    const correlated = new Set(
      [modemUsbLinkIssue?.usb_device_id, modemUsbLinkIssue?.root_net_device_id].filter(
        (id): id is string => Boolean(id),
      ),
    );
    const devices = (advanced?.failing_devices ?? []).filter((device) => !correlated.has(device.device_id));
    return devices.map((device) => {
      const label = PROBLEM_CODE_LABELS[device.problem_code] ?? `Codigo ${device.problem_code}`;
      const name = device.name?.trim() || device.device_class || "Dispositivo desconhecido";
      return {
        title: `${name} com problema no Windows`,
        explanation: `O Windows reportou "${label}" para este dispositivo. Um ciclo de desativar/reativar (o mesmo que fazer manualmente no Gerenciador de Dispositivos) resolve boa parte desses casos - se nao resolver, pode ser necessario reiniciar o computador.`,
        impact: label,
        category: "performance",
        risk: "baixo",
        reversible: true,
        confidence: 0.6,
        reason: `ConfigManagerErrorCode ${device.problem_code} em ${device.device_id}`,
        actionName: "RESTART_PNP_DEVICE",
        actionContext: { deviceId: device.device_id },
      } satisfies Insight;
    });
  }, [telemetry?.advanced, modemUsbLinkIssue]);

  const shadowStorageInsight = useMemo<Insight | null>(() => {
    if (!shadowConsentNeeded || !onResolveShadowConsent) return null;
    return {
      title: t("shadowStorage.cardTitle"),
      explanation: t("shadowStorage.cardBody"),
      impact: t("shadowStorage.cardImpact"),
      category: "limpeza",
      risk: "baixo",
      reversible: true,
      confidence: 0.95,
      reason: t("shadowStorage.cardReason"),
      action: {
        label: t("shadowStorage.cardAuto"),
        onClick: () => void onResolveShadowConsent("auto"),
      },
      secondaryAction: {
        label: t("shadowStorage.cardManual"),
        onClick: () => void onResolveShadowConsent("manual"),
      },
    };
  }, [shadowConsentNeeded, onResolveShadowConsent, t]);

  const visibleInsights = useMemo(() => {
    const local = [
      shadowStorageInsight,
      diskUsageInsight,
      vpnLatencyInsight,
      gameServerLatencyInsight,
      diskNearFullInsight,
      scheduledDefragInsight,
      systemHealthInsight,
      modemUsbLinkInsight,
      ...failingDeviceInsights,
    ].filter((insight): insight is Insight => insight != null);
    const all = [...local, ...insights];
    return all.filter((insight) => !(insightKey(insight) in dismissed));
  }, [
    shadowStorageInsight,
    diskUsageInsight,
    vpnLatencyInsight,
    gameServerLatencyInsight,
    diskNearFullInsight,
    scheduledDefragInsight,
    systemHealthInsight,
    modemUsbLinkInsight,
    failingDeviceInsights,
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
      getNetworkDiagnostics()
        .then(setNetworkDiagnostics)
        .catch(() => undefined);
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

  return (
    <div className="flex flex-col gap-8">
      <header className="flex items-end justify-between gap-4">
        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-2 font-mono text-[11px] uppercase tracking-[0.3em] text-cyan-400/70">
            <Sparkles className="h-3 w-3" />
            {t("insights.eyebrow")}
          </div>
          <h1 className="text-[36px] font-semibold tracking-tight text-slate-50">
            {t("insights.title")}
          </h1>
          <p className="max-w-xl text-sm text-slate-400">
            {t("insights.description")}
          </p>
        </div>
        <div className="flex flex-wrap justify-end gap-2">
          <button
            onClick={openOnWeb}
            className="inline-flex items-center gap-2 rounded-xl border border-cyan-400/30 bg-slate-950/50 px-4 py-2.5 text-sm font-semibold text-cyan-100 transition-all hover:border-cyan-300/60 hover:bg-cyan-400/10"
          >
            <ExternalLink className="h-4 w-4" />
            {t("insights.openInWeb")}
          </button>
          <button
            onClick={generate}
            disabled={loading}
            className="group inline-flex items-center gap-2 rounded-xl border border-cyan-400/40 bg-gradient-to-r from-cyan-500/20 to-violet-500/10 px-4 py-2.5 text-sm font-semibold text-cyan-100 transition-all hover:border-cyan-300/60 hover:shadow-[0_0_25px_-5px_hsl(187_100%_55%/0.7)] disabled:opacity-50"
          >
            <RefreshCw className={`h-4 w-4 ${loading ? "animate-spin" : "transition-transform group-hover:rotate-180"}`} />
            {loading ? t("common.refreshing") : t("common.refresh")}
          </button>
        </div>
      </header>

      {error && visibleInsights.length > 0 && (
        <div className="rounded-xl border border-rose-500/30 bg-rose-500/10 p-4 text-sm text-rose-200">
          {error}
        </div>
      )}

      {actionMessage && (
        <div className="rounded-xl border border-cyan-500/30 bg-cyan-500/10 p-4 text-sm text-cyan-100">
          {actionMessage}
        </div>
      )}

      {loading && visibleInsights.length === 0 ? (
        <div className="grid grid-cols-1 gap-5 md:grid-cols-2">
          {[0, 1, 2, 3].map((i) => (
            <div key={i} className="glass-panel h-44 animate-pulse p-6">
              <div className="h-3 w-24 rounded bg-slate-800/60" />
              <div className="mt-4 h-5 w-2/3 rounded bg-slate-800/60" />
              <div className="mt-3 h-3 w-full rounded bg-slate-800/40" />
              <div className="mt-2 h-3 w-5/6 rounded bg-slate-800/40" />
            </div>
          ))}
        </div>
      ) : error && visibleInsights.length === 0 ? (
        <div className="glass-panel flex flex-col items-center gap-3 rounded-2xl border border-rose-500/20 p-10 text-center">
          <AlertCircle className="h-8 w-8 text-rose-300" />
          <h3 className="text-lg font-semibold text-slate-100">{t("insights.errorTitle")}</h3>
          <p className="max-w-sm text-sm text-slate-400">{error}</p>
          <button
            onClick={generate}
            className="mt-1 inline-flex items-center gap-2 rounded-xl border border-rose-400/30 bg-rose-400/10 px-4 py-2 text-sm font-semibold text-rose-100 transition-all hover:border-rose-300/50 hover:bg-rose-400/15"
          >
            <RefreshCw className="h-4 w-4" />
            {t("insights.errorRetry")}
          </button>
        </div>
      ) : visibleInsights.length === 0 ? (
        <div className="glass-panel flex flex-col items-center gap-2 rounded-2xl border border-cyan-500/10 p-10 text-center">
          <Sparkles className="h-8 w-8 text-cyan-300/60" />
          <h3 className="text-lg font-semibold text-slate-100">{t("insights.emptyTitle")}</h3>
          <p className="max-w-sm text-sm text-slate-400">{t("insights.emptyDescription")}</p>
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-5 md:grid-cols-2">
          {visibleInsights.map((ins) => {
            const m = meta[ins.category] ?? meta.performance;
            const Icon = m.icon;
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
            return (
              <article
                key={key}
                className={`group relative overflow-hidden rounded-2xl border ${m.ring} bg-gradient-to-br ${m.tone} p-6 backdrop-blur-sm transition-all hover:-translate-y-0.5 hover:shadow-[0_20px_40px_-20px_hsl(187_100%_55%/0.4)]`}
              >
                <div className="pointer-events-none absolute -right-12 -top-12 h-40 w-40 rounded-full bg-gradient-to-br from-white/5 to-transparent blur-2xl" />
                <div className="flex items-start justify-between">
                  <div className="grid h-10 w-10 place-items-center rounded-xl border border-white/10 bg-slate-950/60">
                    <Icon className="h-5 w-5 text-cyan-200" />
                  </div>
                  <div className="flex items-center gap-2">
                    <span className={`inline-flex items-center gap-1 rounded-md border px-2 py-0.5 font-mono text-[10px] uppercase tracking-widest ${m.chip}`}>
                      {categoryLabel(ins.category, t)}
                    </span>
                    <button
                      onClick={() => dismissInsight(ins)}
                      title="Dispensar"
                      className="grid h-6 w-6 shrink-0 place-items-center rounded-md border border-white/10 bg-slate-950/60 text-slate-500 transition hover:border-rose-400/40 hover:text-rose-200"
                    >
                      <X className="h-3.5 w-3.5" />
                    </button>
                  </div>
                </div>
                <h3 className="mt-4 text-lg font-semibold tracking-tight text-slate-50">
                  {ins.title}
                </h3>
                <p className="mt-2 text-sm leading-relaxed text-slate-400">{ins.explanation}</p>
                <div className="mt-4 flex items-center gap-2 border-t border-white/5 pt-3">
                  <Brain className="h-3.5 w-3.5 text-cyan-300" />
                  <span className="font-mono text-[11px] uppercase tracking-widest text-slate-500">
                    {t("insights.impact")}
                  </span>
                  <span className="ml-auto font-mono text-sm font-semibold text-gradient-cyber">
                    {ins.impact}
                  </span>
                </div>
                {ins.action && (
                  <div className="mt-3 flex flex-wrap items-center gap-x-4 gap-y-2">
                    <button
                      onClick={ins.action.onClick}
                      className="group/action inline-flex items-center gap-1.5 text-xs font-semibold text-cyan-200 transition hover:text-cyan-100"
                    >
                      {ins.action.label}
                      <ArrowRight className="h-3.5 w-3.5 transition-transform group-hover/action:translate-x-0.5" />
                    </button>
                    {ins.secondaryAction && (
                      <button
                        onClick={ins.secondaryAction.onClick}
                        className="text-xs font-semibold text-slate-400 transition hover:text-slate-200"
                      >
                        {ins.secondaryAction.label}
                      </button>
                    )}
                  </div>
                )}
                {(canRunLocally || canRequestAgent || canRunSecondaryLocally) && (
                  <div className="mt-3 flex flex-wrap gap-2">
                    {canRequestAgent && (
                      <button
                        disabled={busy}
                        onClick={() => void requestAgentApply(ins)}
                        className="inline-flex items-center gap-1.5 rounded-lg border border-cyan-400/40 bg-cyan-400/10 px-3 py-1.5 text-xs font-semibold text-cyan-100 transition hover:bg-cyan-400/15 disabled:opacity-50"
                      >
                        <Bot className="h-3.5 w-3.5" />
                        Deixar o agente fazer
                      </button>
                    )}
                    {canRunLocally && (
                      <button
                        disabled={busy}
                        onClick={() => void applyLocally(ins)}
                        className="inline-flex items-center gap-1.5 rounded-lg border border-emerald-400/40 bg-emerald-400/10 px-3 py-1.5 text-xs font-semibold text-emerald-100 transition hover:bg-emerald-400/15 disabled:opacity-50"
                      >
                        <User className="h-3.5 w-3.5" />
                        {ins.actionLabel ?? "Fazer eu mesmo"}
                      </button>
                    )}
                    {canRunSecondaryLocally && (
                      <button
                        disabled={secondaryBusy}
                        onClick={() => void applySecondaryLocally(ins)}
                        className="inline-flex items-center gap-1.5 rounded-lg border border-rose-400/30 bg-rose-400/10 px-3 py-1.5 text-xs font-semibold text-rose-100 transition hover:bg-rose-400/15 disabled:opacity-50"
                      >
                        {ins.secondaryActionLabel ?? "Desativar"}
                      </button>
                    )}
                  </div>
                )}
              </article>
            );
          })}
        </div>
      )}
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
