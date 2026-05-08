import { ArrowUpRight, Cpu, Gamepad2, ShieldCheck, Sparkles, Wind, Zap } from "lucide-react";
import { TiltCard } from "../TiltCard";
import type { User } from "@/hooks/useAuth";
import type { AgentStatus } from "@/services/tauri/agent";
import { useTelemetry } from "@/hooks/useTelemetry";
import { useI18n } from "@/i18n";

export function Dashboard({
  user,
  status,
  onStartAgent,
  busy,
}: {
  user: User | null;
  status: AgentStatus | null;
  onStartAgent: () => Promise<void>;
  busy: boolean;
}) {
  const { t } = useI18n();
  const track = useTelemetry("dashboard");
  const isAuthenticated = Boolean(status?.authenticated);
  const isReady = Boolean(status?.authenticated && status.registered);
  const statusTitle = !isAuthenticated
    ? t("agent.status.waitingLogin")
    : isReady
      ? t("dashboard.operational")
      : t("agent.status.hardwarePending");

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
          {t("dashboard.description")}
        </p>
      </header>

      <TiltCard className="h-[300px]" intensity={5}>
        <div className="relative flex h-full flex-col justify-between overflow-hidden p-8">
          <div className="pointer-events-none absolute -right-16 -top-16 h-64 w-64 rounded-full bg-gradient-to-br from-cyan-500/30 via-violet-500/20 to-transparent blur-3xl" />
          <div className="pointer-events-none absolute -left-10 bottom-0 h-40 w-40 rounded-full bg-gradient-to-tr from-fuchsia-500/15 to-transparent blur-3xl" />

          <div className="relative flex items-start justify-between">
            <div>
              <div className="font-mono text-[11px] uppercase tracking-[0.3em] text-cyan-400/80">
                {t("dashboard.statusLabel")}
              </div>
              <div className="mt-3 flex items-center gap-3">
                <span className="relative flex h-3 w-3">
                  <span className={`absolute inline-flex h-full w-full animate-ping rounded-full ${isReady ? "bg-cyan-400" : "bg-amber-400"} opacity-60`} />
                  <span className={`relative inline-flex h-3 w-3 rounded-full ${isReady ? "bg-cyan-400 shadow-[0_0_12px_hsl(187_100%_60%)]" : "bg-amber-400 shadow-[0_0_12px_hsl(40_100%_60%)]"}`} />
                </span>
                <span className="text-3xl font-semibold tracking-tight text-slate-50">
                  {statusTitle}
                </span>
                <span className="rounded-md border border-emerald-400/30 bg-emerald-400/10 px-2 py-0.5 font-mono text-[10px] uppercase tracking-widest text-emerald-300">
                  {isReady ? t("dashboard.excellent") : t("common.pending")}
                </span>
              </div>
              <p className="mt-2 text-sm text-slate-400">
                {t("dashboard.lastCheck", { seconds: 2 })}
              </p>
            </div>
            <div className="flex flex-col items-end gap-2">
              <div className="grid h-12 w-12 place-items-center rounded-xl border border-cyan-400/30 bg-cyan-500/10">
                <ShieldCheck className="h-6 w-6 text-cyan-300" />
              </div>
              <div className="font-mono text-[10px] uppercase tracking-[0.25em] text-slate-500">
                {t("dashboard.footprint")}
              </div>
            </div>
          </div>

          <div className="relative grid grid-cols-3 gap-3">
            <Stat icon={<Zap className="h-3.5 w-3.5" />} label={t("dashboard.optimize")} value="92%" delta="+4%" positive />
            <Stat icon={<Cpu className="h-3.5 w-3.5" />} label={t("dashboard.processes")} value="142" delta="-8" positive />
            <Stat icon={<Wind className="h-3.5 w-3.5" />} label={t("dashboard.latency")} value="12ms" delta="-2ms" positive />
          </div>

          <div className="relative flex items-center justify-between">
            <button
              disabled={busy || !isReady}
              onClick={() => {
                track("game_mode_clicked");
                void onStartAgent();
              }}
              className="group inline-flex items-center gap-2.5 rounded-xl border border-cyan-400/40 bg-gradient-to-r from-cyan-500/20 to-violet-500/10 px-6 py-3 text-sm font-semibold text-cyan-100 transition-all duration-300 hover:border-cyan-300/60 hover:from-cyan-500/30 hover:shadow-[0_0_30px_-5px_hsl(187_100%_55%/0.7)] disabled:opacity-50"
            >
              <Gamepad2 className="h-4 w-4 transition-transform group-hover:-rotate-12" />
              {t("dashboard.gameMode")}
              <ArrowUpRight className="h-4 w-4 opacity-60 transition-transform group-hover:translate-x-0.5 group-hover:-translate-y-0.5" />
            </button>
            <Sparkline />
          </div>
        </div>
      </TiltCard>

      <div className="grid grid-cols-1 gap-5 md:grid-cols-3">
        <ActionCard title={t("dashboard.clearCache")} desc={t("dashboard.clearCacheDesc")} accent="cyan" />
        <ActionCard title={t("dashboard.optimizeBoot")} desc={t("dashboard.optimizeBootDesc")} accent="violet" />
        <ActionCard title={t("dashboard.quietMode")} desc={t("dashboard.quietModeDesc")} accent="fuchsia" />
      </div>
    </div>
  );
}

function Stat({ icon, label, value, delta, positive }: { icon: React.ReactNode; label: string; value: string; delta: string; positive: boolean }) {
  return (
    <div className="rounded-xl border border-cyan-500/10 bg-slate-950/50 p-4 backdrop-blur-sm">
      <div className="flex items-center justify-between text-slate-500">
        <div className="flex items-center gap-1.5 font-mono text-[10px] uppercase tracking-widest">
          {icon}
          {label}
        </div>
        <span className={positive ? "font-mono text-[10px] text-emerald-400" : "font-mono text-[10px] text-rose-400"}>
          {delta}
        </span>
      </div>
      <div className="mt-2 text-2xl font-semibold tracking-tight text-slate-50">{value}</div>
    </div>
  );
}

function Sparkline() {
  // Pre-baked path for visual charm — pure SVG, ~0KB runtime cost
  return (
    <svg viewBox="0 0 120 36" className="h-9 w-32 opacity-90">
      <defs>
        <linearGradient id="sparkGrad" x1="0" x2="1" y1="0" y2="0">
          <stop offset="0%" stopColor="hsl(187 100% 60%)" />
          <stop offset="100%" stopColor="hsl(265 90% 70%)" />
        </linearGradient>
        <linearGradient id="sparkFill" x1="0" x2="0" y1="0" y2="1">
          <stop offset="0%" stopColor="hsl(187 100% 55% / 0.4)" />
          <stop offset="100%" stopColor="hsl(187 100% 55% / 0)" />
        </linearGradient>
      </defs>
      <path d="M0,28 L12,22 L24,26 L36,16 L48,20 L60,10 L72,14 L84,6 L96,12 L108,4 L120,9 L120,36 L0,36 Z" fill="url(#sparkFill)" />
      <path d="M0,28 L12,22 L24,26 L36,16 L48,20 L60,10 L72,14 L84,6 L96,12 L108,4 L120,9" fill="none" stroke="url(#sparkGrad)" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function ActionCard({ title, desc, accent }: { title: string; desc: string; accent: "cyan" | "violet" | "fuchsia" }) {
  const { t } = useI18n();
  const track = useTelemetry("dashboard");
  const tints: Record<string, string> = {
    cyan: "from-cyan-500/15 to-cyan-500/0 border-cyan-400/20 hover:border-cyan-400/50",
    violet: "from-violet-500/15 to-violet-500/0 border-violet-400/20 hover:border-violet-400/50",
    fuchsia: "from-fuchsia-500/15 to-fuchsia-500/0 border-fuchsia-400/20 hover:border-fuchsia-400/50",
  };
  const dot: Record<string, string> = {
    cyan: "bg-cyan-400",
    violet: "bg-violet-400",
    fuchsia: "bg-fuchsia-400",
  };
  return (
    <button
      onClick={() => track("quick_action_clicked", { action: title })}
      className={`group relative overflow-hidden rounded-2xl border bg-gradient-to-br ${tints[accent]} p-5 text-left transition-all duration-300`}
    >
      <div className="flex items-center gap-2">
        <span className={`h-1.5 w-1.5 rounded-full ${dot[accent]} shadow-[0_0_8px_currentColor]`} />
        <span className="font-mono text-[10px] uppercase tracking-[0.25em] text-slate-400">{t("dashboard.quickAction")}</span>
      </div>
      <div className="mt-3 flex items-center justify-between">
        <div>
          <div className="text-base font-semibold text-slate-100">{title}</div>
          <div className="text-xs text-slate-500">{desc}</div>
        </div>
        <ArrowUpRight className="h-4 w-4 text-slate-500 transition-all group-hover:translate-x-0.5 group-hover:-translate-y-0.5 group-hover:text-slate-200" />
      </div>
    </button>
  );
}
