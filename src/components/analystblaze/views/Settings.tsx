import { Brain, ExternalLink, Globe, LogOut, Moon, Shield, Sparkles, Sun, User as UserIcon, Zap } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTheme } from "@/hooks/useTheme";
import { useTelemetry } from "@/hooks/useTelemetry";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { AgentMessage, User } from "@/hooks/useAuth";
import type { AgentStatus, AgentTelemetrySample } from "@/services/tauri/agent";
import { getTelemetryQueueSize, isTelemetryEnabled, setTelemetryEnabled } from "@/services/telemetry";
import { localeLabels, type Locale, useI18n } from "@/i18n";

export function Settings({
  user,
  status,
  message,
  busy,
  onLogin,
  onLogout,
  onStartAgent,
  onSetTelemetryMode,
  onCollectSample,
}: {
  user: User | null;
  status: AgentStatus | null;
  message: AgentMessage;
  busy: boolean;
  onLogin: () => Promise<void>;
  onLogout: () => Promise<void>;
  onStartAgent: () => Promise<void>;
  onSetTelemetryMode: (mode: "normal" | "realtime") => Promise<void>;
  onCollectSample: () => Promise<AgentTelemetrySample>;
}) {
  const { theme, setTheme } = useTheme();
  const { t, locale, setLocale } = useI18n();
  const track = useTelemetry("settings");
  const didMountPreferences = useRef(false);
  const [telem, setTelem] = useState(isTelemetryEnabled);
  const [queueSize, setQueueSize] = useState(getTelemetryQueueSize);
  const [adaptive, setAdaptive] = useState<boolean>(() => {
    try { return localStorage.getItem("analystblaze.adaptive") === "1"; } catch { return false; }
  });

  useEffect(() => {
    try { localStorage.setItem("analystblaze.adaptive", adaptive ? "1" : "0"); } catch {}
    if (didMountPreferences.current) {
      track("adaptive_mode_changed", { enabled: adaptive });
    }
  }, [adaptive]);

  useEffect(() => {
    setTelemetryEnabled(telem);
    if (didMountPreferences.current) {
      track("ui_telemetry_preference_changed", { enabled: telem });
    }
  }, [telem]);

  useEffect(() => {
    didMountPreferences.current = true;
  }, []);

  useEffect(() => {
    const id = window.setInterval(() => setQueueSize(getTelemetryQueueSize()), 10_000);
    return () => window.clearInterval(id);
  }, []);

  const dark = theme === "dark";
  const isReady = Boolean(status?.authenticated && status.registered);

  return (
    <div className="flex flex-col gap-8">
      <header className="flex flex-col gap-2">
        <div className="font-mono text-[11px] uppercase tracking-[0.3em] text-cyan-400/70">
          {t("settings.eyebrow")}
        </div>
        <h1 className="text-[36px] font-semibold tracking-tight text-slate-50">
          {t("settings.title")}
        </h1>
        <p className="text-sm text-slate-400">{t("settings.description")}</p>
      </header>

      <section className="glass-panel cyber-glow p-6">
        <div className="flex items-center gap-2 pb-4">
          {dark ? <Moon className="h-3.5 w-3.5 text-cyan-300" /> : <Sun className="h-3.5 w-3.5 text-cyan-300" />}
          <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-cyan-400/80">{t("settings.appearance")}</h2>
        </div>
        <div className="flex flex-col gap-3">
          <Row
            label={t("settings.darkTheme")}
            description={t("settings.darkThemeDesc")}
            control={<Switch checked={dark} onCheckedChange={(value) => {
              setTheme(value ? "dark" : "light");
              track("theme_changed", { theme: value ? "dark" : "light" });
            }} />}
          />
          <Row
            label={t("settings.language")}
            description={t("settings.languageDesc")}
            control={
              <Select value={locale} onValueChange={(value) => {
                setLocale(value as Locale);
                track("locale_changed", { locale: value });
              }}>
                <SelectTrigger className="w-40 border-cyan-500/20 bg-slate-950/60 font-mono text-xs">
                  <Globe className="mr-2 h-3.5 w-3.5 text-cyan-300" />
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {(Object.entries(localeLabels) as [Locale, string][]).map(([value, label]) => (
                    <SelectItem key={value} value={value}>{label}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            }
          />
        </div>
      </section>

      <section className={`relative overflow-hidden glass-panel cyber-glow p-6 transition-all ${adaptive ? "ring-1 ring-cyan-400/40 shadow-[0_0_40px_-10px_hsl(187_100%_55%/0.6)]" : ""}`}>
        {adaptive && (
          <div className="pointer-events-none absolute -right-16 -top-16 h-48 w-48 rounded-full bg-gradient-to-br from-cyan-500/30 via-violet-500/20 to-transparent blur-3xl" />
        )}
        <div className="relative flex items-center gap-2 pb-4">
          <Brain className="h-3.5 w-3.5 text-cyan-300" />
          <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-cyan-400/80">{t("settings.adaptive")}</h2>
          {adaptive && (
            <span className="ml-2 inline-flex items-center gap-1 rounded-md border border-cyan-400/40 bg-cyan-500/10 px-2 py-0.5 font-mono text-[9px] uppercase tracking-widest text-cyan-200">
              <span className="h-1 w-1 animate-pulse rounded-full bg-cyan-300 shadow-[0_0_8px_currentColor]" />
              {t("settings.adaptiveActive")}
            </span>
          )}
        </div>

        <div className="relative flex items-start justify-between gap-4 rounded-xl border border-cyan-500/15 bg-slate-950/40 p-5">
          <div className="flex items-start gap-3">
            <div className="grid h-10 w-10 shrink-0 place-items-center rounded-xl border border-cyan-400/30 bg-gradient-to-br from-cyan-500/20 to-violet-500/10">
              <Sparkles className="h-4 w-4 text-cyan-300" />
            </div>
            <div>
              <div className="text-sm font-semibold text-slate-100">{t("settings.adaptiveDecision")}</div>
              <div className="mt-0.5 max-w-md text-xs text-slate-500">
                {t("settings.adaptiveDesc")}
              </div>
              <div className="mt-3 flex flex-wrap gap-1.5">
                {["energy", "processes", "network", "fans"].map((tag) => (
                  <span key={tag} className="rounded-md border border-cyan-500/15 bg-slate-950/60 px-2 py-0.5 font-mono text-[9px] uppercase tracking-widest text-slate-400">
                    {t(`settings.tags.${tag}`)}
                  </span>
                ))}
              </div>
            </div>
          </div>
          <Switch checked={adaptive} onCheckedChange={setAdaptive} />
        </div>

        {adaptive && (
          <div className="relative mt-3 flex items-center gap-2 rounded-lg border border-amber-400/20 bg-amber-400/5 px-3 py-2 text-[11px] text-amber-200/80">
            <Zap className="h-3 w-3" />
            {t("settings.adaptiveWarning")}
          </div>
        )}
      </section>

      <section className="glass-panel cyber-glow p-6">
        <div className="flex items-center gap-2 pb-4">
          <Shield className="h-3.5 w-3.5 text-cyan-300" />
          <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-cyan-400/80">{t("settings.privacy")}</h2>
        </div>
        <Row
          label={t("settings.uiTelemetry")}
          description={t("settings.uiTelemetryDesc")}
          control={<Switch checked={telem} onCheckedChange={setTelem} />}
        />
        <div className="mt-3 rounded-xl border border-cyan-500/10 bg-slate-950/40 p-4 font-mono text-[11px] uppercase tracking-widest text-slate-500">
          {t("settings.telemetryBuffer")}: {queueSize}
        </div>
      </section>

      <section className="glass-panel cyber-glow p-6">
        <div className="flex items-center gap-2 pb-4">
          <UserIcon className="h-3.5 w-3.5 text-cyan-300" />
          <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-cyan-400/80">{t("settings.account")}</h2>
        </div>

        {user ? (
          <div className="flex items-center gap-4 rounded-xl border border-cyan-500/15 bg-gradient-to-br from-slate-950/60 to-slate-900/30 p-5">
            <div className="grid h-14 w-14 place-items-center rounded-full border border-cyan-400/30 bg-gradient-to-br from-cyan-500/30 to-violet-500/20 text-lg font-semibold text-cyan-100 shadow-[0_0_20px_-5px_hsl(187_100%_55%/0.7)]">
              {user.name.slice(0, 1).toUpperCase()}
            </div>
            <div className="flex-1">
              <div className="text-base font-semibold text-slate-50">{user.name}</div>
              <div className="mt-0.5 font-mono text-[10px] uppercase tracking-widest text-cyan-300">
                {t("sidebar.currentPlan")}: {planLabel(user.plan)}
              </div>
              <div className="mt-0.5 font-mono text-[11px] text-slate-500">
                {t("sidebar.session")} - {user.sessionId.slice(0, 8)}...{user.sessionId.slice(-4)}
              </div>
              <div className="mt-2 inline-flex items-center gap-1.5 rounded-md border border-emerald-400/30 bg-emerald-400/10 px-2 py-0.5 font-mono text-[10px] uppercase tracking-widest text-emerald-300">
                <span className="h-1 w-1 rounded-full bg-emerald-400" />
                {isReady ? t("common.online") : t("common.pending")}
              </div>
            </div>
            {!user.hasPaidPlan && (
              <button
                onClick={() => window.open(import.meta.env.VITE_ANALYSTBLAZE_BILLING_URL ?? "https://analystblaze.app/billing", "_blank", "noopener,noreferrer")}
                className="inline-flex items-center gap-2 rounded-xl border border-cyan-400/40 bg-cyan-400/10 px-4 py-2.5 text-sm font-medium text-cyan-100 transition-all hover:border-cyan-300/60 hover:bg-cyan-400/15"
              >
                <ExternalLink className="h-4 w-4" />
                {t("sidebar.becomePro")}
              </button>
            )}
            <button
              onClick={() => void onLogout()}
              className="inline-flex items-center gap-2 rounded-xl border border-rose-500/30 bg-rose-500/10 px-4 py-2.5 text-sm font-medium text-rose-300 transition-all hover:bg-rose-500/20 hover:border-rose-400/50"
            >
              <LogOut className="h-4 w-4" />
              {t("common.logout")}
            </button>
          </div>
        ) : (
          <div className="flex flex-col items-start gap-4 rounded-xl border border-dashed border-cyan-500/20 bg-slate-950/30 p-5">
            <p className="text-sm text-slate-400">
              {t("settings.loggedOut")}
            </p>
            <button
              onClick={() => void onLogin()}
              className="group inline-flex items-center gap-2.5 rounded-xl border border-cyan-400/40 bg-gradient-to-r from-cyan-500/20 to-violet-500/10 px-5 py-2.5 text-sm font-semibold text-cyan-100 transition-all duration-300 hover:border-cyan-300/60 hover:shadow-[0_0_25px_-5px_hsl(187_100%_55%/0.7)]"
            >
              {t("settings.webLogin")}
              <ExternalLink className="h-4 w-4 transition-transform group-hover:translate-x-0.5 group-hover:-translate-y-0.5" />
            </button>
          </div>
        )}
      </section>

      <section className="glass-panel cyber-glow p-6">
        <div className="flex items-center gap-2 pb-4">
          <Zap className="h-3.5 w-3.5 text-cyan-300" />
          <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-cyan-400/80">{t("settings.agentPanel")}</h2>
        </div>
        <div className="grid grid-cols-1 gap-3 md:grid-cols-4">
          <MiniMetric label={t("agent.status.auth")} value={status?.authenticated ? "OK" : t("common.pending")} />
          <MiniMetric label={t("agent.status.hardware")} value={status?.registered ? t("common.registered") : t("common.pending")} />
          <MiniMetric label={t("agent.status.telemetry")} value={status?.mode ?? t("common.stopped")} />
          <MiniMetric label={t("agent.status.api")} value={status?.api_base_url ?? t("common.unavailable")} compact />
        </div>
        {status?.hw_id && (
          <div className="mt-3 rounded-xl border border-cyan-500/10 bg-slate-950/40 p-4 text-xs text-slate-400">
            <span className="mb-1 block font-mono text-[10px] uppercase tracking-widest text-slate-500">{t("agent.status.hwId")}</span>
            <span className="break-all">{status.hw_id}</span>
          </div>
        )}
        <div className="mt-4 flex flex-wrap gap-2">
          <ActionButton disabled={busy || !isReady} onClick={onStartAgent}>{t("settings.startAgent")}</ActionButton>
          <ActionButton disabled={busy || !isReady} onClick={() => onSetTelemetryMode("realtime")}>{t("settings.forceRealtime")}</ActionButton>
          <ActionButton disabled={busy || !isReady} onClick={() => onSetTelemetryMode("normal")}>{t("settings.restoreNormal")}</ActionButton>
          <ActionButton disabled={busy} onClick={onCollectSample}>{t("settings.collectSample")}</ActionButton>
        </div>
        <p className="mt-4 text-sm text-slate-400">{t(message.key, message.params)}</p>
      </section>
    </div>
  );
}

function planLabel(plan: string) {
  const normalized = plan.trim().toLowerCase();
  if (!normalized || normalized === "free") return "Starter";
  return normalized.slice(0, 1).toUpperCase() + normalized.slice(1);
}

function Row({
  label,
  description,
  control,
}: {
  label: string;
  description: string;
  control: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-4 rounded-xl border border-cyan-500/10 bg-slate-950/40 p-4">
      <div>
        <div className="text-sm font-medium text-slate-100">{label}</div>
        <div className="mt-0.5 text-xs text-slate-500">{description}</div>
      </div>
      {control}
    </div>
  );
}

function MiniMetric({ label, value, compact = false }: { label: string; value: string; compact?: boolean }) {
  return (
    <article className="min-h-24 rounded-xl border border-cyan-500/10 bg-slate-950/40 p-4">
      <span className="block font-mono text-[10px] uppercase tracking-widest text-slate-500">{label}</span>
      <strong className={`mt-2 block break-words text-slate-100 ${compact ? "text-xs" : "text-lg"}`}>{value}</strong>
    </article>
  );
}

function ActionButton({
  disabled,
  onClick,
  children,
}: {
  disabled: boolean;
  onClick: () => Promise<unknown>;
  children: React.ReactNode;
}) {
  return (
    <button
      disabled={disabled}
      onClick={() => void onClick().catch(() => undefined)}
      className="rounded-xl border border-cyan-400/40 bg-gradient-to-r from-cyan-500/20 to-violet-500/10 px-4 py-2.5 text-sm font-semibold text-cyan-100 transition-all hover:border-cyan-300/60 disabled:opacity-50"
    >
      {children}
    </button>
  );
}
