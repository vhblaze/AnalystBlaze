import { useEffect, useMemo, useState } from "react";
import { ArrowRight, Bot, Brain, Cpu, Droplets, ExternalLink, RefreshCw, Sparkles, User, Wind, X, Zap } from "lucide-react";
import { fetchInsights, type Insight } from "@/services/insights";
import { useI18n } from "@/i18n";
import { useTelemetry } from "@/hooks/useTelemetry";
import { isTauriRuntime, openAgentInsights, type AgentTelemetrySnapshot } from "@/services/tauri/agent";

const DISK_USAGE_WARNING_THRESHOLD_PERCENT = 80;
const DISMISS_STORAGE_KEY = "analystblaze.dismissedInsights";
const DISMISS_TTL_MS = 24 * 60 * 60 * 1000;
/** The server only ever pairs a card with an actionName from its own small
 * validated allowlist (see safe_action_policy.py) - these are the ones the
 * desktop already knows how to run locally, so "I'll do it myself" has
 * something real to call. Anything else server-side is display-only for now. */
const LOCALLY_EXECUTABLE_ACTIONS = new Set(["APPLY_GAME_MODE", "EMPTY_TEMP"]);

function insightKey(insight: Pick<Insight, "category" | "actionName" | "title">): string {
  return `${insight.category}:${insight.actionName ?? insight.title}`;
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
  onOpenDiskUsage,
  onApplyInsightActionLocally,
  onRequestAgentApplyInsight,
}: {
  telemetry?: AgentTelemetrySnapshot | null;
  onOpenDiskUsage?: () => void;
  /** "I'll do it myself" - runs the action right now, locally, with the
   * same confirmation dialog its dedicated button elsewhere already uses. */
  onApplyInsightActionLocally?: (actionName: string) => Promise<unknown>;
  /** "Let the agent do it" - enqueues the action server-side; the agent
   * applies it on its own next sync cycle (see applyInsightAction). */
  onRequestAgentApplyInsight?: (actionName: string, title: string, reason: string) => Promise<unknown>;
}) {
  const { t, locale } = useI18n();
  const track = useTelemetry("insights");
  const [insights, setInsights] = useState<Insight[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState<Record<string, number>>(() => loadDismissed());
  const [actionBusyKey, setActionBusyKey] = useState<string | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);

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

  const applyLocally = async (insight: Insight) => {
    if (!insight.actionName || !onApplyInsightActionLocally) return;
    const key = insightKey(insight);
    setActionBusyKey(key);
    setActionMessage(null);
    try {
      await onApplyInsightActionLocally(insight.actionName);
      track("insight_action_applied_locally", { actionName: insight.actionName });
      dismissInsight(insight);
    } catch (e: any) {
      setActionMessage(e?.message ?? "Falha ao aplicar a acao.");
    } finally {
      setActionBusyKey(null);
    }
  };

  const requestAgentApply = async (insight: Insight) => {
    if (!insight.actionName || !onRequestAgentApplyInsight) return;
    const key = insightKey(insight);
    setActionBusyKey(key);
    setActionMessage(null);
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

  const visibleInsights = useMemo(() => {
    const all = diskUsageInsight ? [diskUsageInsight, ...insights] : insights;
    return all.filter((insight) => !(insightKey(insight) in dismissed));
  }, [diskUsageInsight, insights, dismissed]);

  const generate = async () => {
    setLoading(true);
    setError(null);
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

      {error && (
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
            const canRequestAgent = Boolean(ins.actionName && onRequestAgentApplyInsight);
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
                  <button
                    onClick={ins.action.onClick}
                    className="group/action mt-3 inline-flex items-center gap-1.5 text-xs font-semibold text-cyan-200 transition hover:text-cyan-100"
                  >
                    {ins.action.label}
                    <ArrowRight className="h-3.5 w-3.5 transition-transform group-hover/action:translate-x-0.5" />
                  </button>
                )}
                {(canRunLocally || canRequestAgent) && (
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
                        Fazer eu mesmo
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
