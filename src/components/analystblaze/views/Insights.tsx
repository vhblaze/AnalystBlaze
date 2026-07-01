import { useEffect, useState } from "react";
import { Brain, Cpu, Droplets, ExternalLink, RefreshCw, Sparkles, Wind, Zap } from "lucide-react";
import { fetchInsights, type Insight } from "@/services/insights";
import { useI18n } from "@/i18n";
import { useTelemetry } from "@/hooks/useTelemetry";
import { isTauriRuntime, openAgentInsights } from "@/services/tauri/agent";

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

export function Insights() {
  const { t, locale } = useI18n();
  const track = useTelemetry("insights");
  const [insights, setInsights] = useState<Insight[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

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

      {loading && insights.length === 0 ? (
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
          {insights.map((ins, i) => {
            const m = meta[ins.category] ?? meta.performance;
            const Icon = m.icon;
            return (
              <article
                key={i}
                className={`group relative overflow-hidden rounded-2xl border ${m.ring} bg-gradient-to-br ${m.tone} p-6 backdrop-blur-sm transition-all hover:-translate-y-0.5 hover:shadow-[0_20px_40px_-20px_hsl(187_100%_55%/0.4)]`}
              >
                <div className="pointer-events-none absolute -right-12 -top-12 h-40 w-40 rounded-full bg-gradient-to-br from-white/5 to-transparent blur-2xl" />
                <div className="flex items-start justify-between">
                  <div className="grid h-10 w-10 place-items-center rounded-xl border border-white/10 bg-slate-950/60">
                    <Icon className="h-5 w-5 text-cyan-200" />
                  </div>
                  <span className={`inline-flex items-center gap-1 rounded-md border px-2 py-0.5 font-mono text-[10px] uppercase tracking-widest ${m.chip}`}>
                    {categoryLabel(ins.category, t)}
                  </span>
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
