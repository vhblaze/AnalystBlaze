import { Battery, Brain, Check, DownloadCloud, ExternalLink, Eye, Globe, History, LogOut, Moon, RefreshCw, Shield, Sparkles, Sun, Trash2, User as UserIcon, Zap } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { isUpdateDismissedNow, useUpdater } from "@/hooks/useUpdater";
import { useTelemetry } from "@/hooks/useTelemetry";
import { useTheme } from "@/hooks/useTheme";
import { canUseAutomaticGameMode, type AgentMessage, type User } from "@/hooks/useAuth";
import { localeLabels, type Locale, useI18n } from "@/i18n";
import {
  getLocalAiPolicy,
  saveLocalAiPolicy,
  type AgentStatus,
  type AgentTelemetrySample,
  type LocalAiPolicy,
} from "@/services/tauri/agent";
import { getTelemetryQueueSize, isTelemetryEnabled, setTelemetryEnabled } from "@/services/telemetry";

export function Settings({
  user,
  status,
  message,
  busy,
  onLogin,
  onLogout,
  onOpenAccountSettings,
  onOpenBilling,
  onStartAgent,
  onCollectSample,
  onOpenHistory,
}: {
  user: User | null;
  status: AgentStatus | null;
  message: AgentMessage;
  busy: boolean;
  onLogin: () => Promise<void>;
  onLogout: () => Promise<void>;
  onOpenAccountSettings: () => Promise<void>;
  onOpenBilling: () => Promise<void>;
  onStartAgent: () => Promise<void>;
  onCollectSample: () => Promise<AgentTelemetrySample>;
  onOpenHistory?: () => void;
}) {
  const { theme, setTheme } = useTheme();
  const { t, locale, setLocale } = useI18n();
  const track = useTelemetry("settings");
  const updater = useUpdater();
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const checkForUpdatesNow = () => {
    setCheckingUpdate(true);
    updater
      .check()
      .then((next) => track("update_check_requested", { available: next.available }))
      .catch(() => undefined)
      .finally(() => setCheckingUpdate(false));
  };
  const didMountPreferences = useRef(false);
  const [telem, setTelem] = useState(isTelemetryEnabled);
  const [queueSize, setQueueSize] = useState(getTelemetryQueueSize);
  const [onboardingDone, setOnboardingDone] = useState<boolean>(() => {
    try { return localStorage.getItem("analystblaze.onboarding.done") === "1"; } catch { return false; }
  });
  const [highContrast, setHighContrast] = useState<boolean>(() => {
    try { return localStorage.getItem("analystblaze.accessibility.highContrast") === "1"; } catch { return false; }
  });
  const [reduceMotion, setReduceMotion] = useState<boolean>(() => {
    try { return localStorage.getItem("analystblaze.accessibility.reduceMotion") === "1"; } catch { return false; }
  });
  const [adaptive, setAdaptive] = useState<boolean>(() => {
    try { return localStorage.getItem("analystblaze.adaptive") === "1"; } catch { return false; }
  });
  const [aiPolicy, setAiPolicy] = useState<LocalAiPolicy>(DEFAULT_LOCAL_AI_POLICY);

  useEffect(() => {
    getLocalAiPolicy()
      .then((policy) => {
        setAiPolicy(policy);
        setAdaptive(policy.enabled);
      })
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    try { localStorage.setItem("analystblaze.adaptive", adaptive ? "1" : "0"); } catch {}
    if (didMountPreferences.current) {
      track("adaptive_mode_changed", { enabled: adaptive });
    }
  }, [adaptive, track]);

  useEffect(() => {
    setTelemetryEnabled(telem);
    if (didMountPreferences.current) {
      track("ui_telemetry_preference_changed", { enabled: telem });
    }
  }, [telem, track]);

  useEffect(() => {
    didMountPreferences.current = true;
  }, []);

  useEffect(() => {
    const id = window.setInterval(() => setQueueSize(getTelemetryQueueSize()), 10_000);
    return () => window.clearInterval(id);
  }, []);

  useEffect(() => {
    document.documentElement.classList.toggle("ab-high-contrast", highContrast);
    try { localStorage.setItem("analystblaze.accessibility.highContrast", highContrast ? "1" : "0"); } catch {}
  }, [highContrast]);

  useEffect(() => {
    document.documentElement.classList.toggle("ab-reduce-motion", reduceMotion);
    try { localStorage.setItem("analystblaze.accessibility.reduceMotion", reduceMotion ? "1" : "0"); } catch {}
  }, [reduceMotion]);

  const updateAiPolicy = (patch: Partial<LocalAiPolicy>) => {
    const nextPolicy = { ...aiPolicy, ...patch };
    setAiPolicy(nextPolicy);
    setAdaptive(nextPolicy.enabled);
    saveLocalAiPolicy(nextPolicy)
      .then((saved) => {
        setAiPolicy(saved);
        setAdaptive(saved.enabled);
        track("local_ai_policy_saved", { enabled: saved.enabled, maxRisk: saved.max_risk });
      })
      .catch(() => undefined);
  };

  const finishOnboarding = () => {
    setOnboardingDone(true);
    try { localStorage.setItem("analystblaze.onboarding.done", "1"); } catch {}
    track("onboarding_dismissed");
  };

  const dark = theme === "dark";
  const isReady = Boolean(status?.authenticated && status.registered);
  const automaticGameModeAllowed = canUseAutomaticGameMode(status);

  return (
    <div className="flex flex-col gap-8">
      <header className="flex flex-col gap-2">
        <div className="font-mono text-[11px] uppercase tracking-[0.3em] text-cyan-400/70">
          {t("settings.eyebrow")}
        </div>
        <h1 className="text-[36px] font-semibold tracking-tight text-slate-50">
          {t("settings.title")}
        </h1>
        <p className="max-w-3xl text-sm text-slate-400">{t("settings.description")}</p>
      </header>

      {!onboardingDone && (
        <section className="glass-panel cyber-glow p-6" role="region" aria-label={t("settings.onboardingTitle")}>
          <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
            <div className="flex items-start gap-3">
              <div className="grid h-10 w-10 shrink-0 place-items-center rounded-xl border border-cyan-400/30 bg-cyan-500/10">
                <Check className="h-4 w-4 text-cyan-300" />
              </div>
              <div>
                <h2 className="text-sm font-semibold text-slate-100">{t("settings.onboardingTitle")}</h2>
                <p className="mt-1 max-w-2xl text-xs leading-relaxed text-slate-400">
                  {t("settings.onboardingDesc")}
                </p>
                <div className="mt-3 flex flex-wrap gap-1.5">
                  {["login", "telemetry", "restore", "privacy"].map((step) => (
                    <span key={step} className="rounded-md border border-cyan-500/15 bg-slate-950/60 px-2 py-0.5 font-mono text-[9px] uppercase tracking-widest text-slate-400">
                      {t(`settings.onboardingSteps.${step}`)}
                    </span>
                  ))}
                </div>
              </div>
            </div>
            <button
              onClick={finishOnboarding}
              className="rounded-xl border border-cyan-400/40 bg-cyan-400/10 px-4 py-2.5 text-sm font-medium text-cyan-100 transition-all hover:border-cyan-300/60 hover:bg-cyan-400/15"
            >
              {t("settings.onboardingDone")}
            </button>
          </div>
        </section>
      )}

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

      <section className={`glass-panel cyber-glow p-6 transition-all ${adaptive ? "ring-1 ring-cyan-400/40" : ""}`}>
        <div className="flex items-center gap-2 pb-4">
          <Brain className="h-3.5 w-3.5 text-cyan-300" />
          <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-cyan-400/80">{t("settings.adaptive")}</h2>
          {adaptive && (
            <span className="ml-2 inline-flex items-center gap-1 rounded-md border border-cyan-400/40 bg-cyan-500/10 px-2 py-0.5 font-mono text-[9px] uppercase tracking-widest text-cyan-200">
              <span className="h-1 w-1 animate-pulse rounded-full bg-cyan-300" />
              {t("settings.adaptiveActive")}
            </span>
          )}
        </div>

        <div className="flex items-start justify-between gap-4 rounded-xl border border-cyan-500/15 bg-slate-950/40 p-5">
          <div className="flex items-start gap-3">
            <div className="grid h-10 w-10 shrink-0 place-items-center rounded-xl border border-cyan-400/30 bg-cyan-500/10">
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
          <Switch checked={aiPolicy.enabled} onCheckedChange={(enabled) => updateAiPolicy({ enabled, agent_mode: enabled ? "automatic" : "manual" })} />
        </div>

        {adaptive && (
          <div className="mt-3 flex items-center gap-2 rounded-lg border border-amber-400/20 bg-amber-400/5 px-3 py-2 text-[11px] text-amber-200/80">
            <Zap className="h-3 w-3" />
            {t("settings.adaptiveWarning")}
          </div>
        )}
        {!automaticGameModeAllowed && (
          <div className="mt-3 flex items-center gap-2 rounded-lg border border-cyan-400/20 bg-cyan-400/5 px-3 py-2 text-[11px] text-cyan-100/80">
            <Shield className="h-3 w-3" />
            Automacoes do Agente Local ficam disponiveis nos planos Pro e Family. No Starter, o PC Limpo continua manual.
          </div>
        )}

        <div className="mt-4 grid gap-3 md:grid-cols-2">
          <Row
            label={t("settings.aiAutoGame")}
            description={t("settings.aiAutoGameDesc")}
            control={<Switch disabled={!automaticGameModeAllowed} checked={automaticGameModeAllowed && aiPolicy.auto_game_mode} onCheckedChange={(auto_game_mode) => updateAiPolicy({ auto_game_mode })} />}
          />
          <Row
            label={t("settings.aiAutoPcClean")}
            description={t("settings.aiAutoPcCleanDesc")}
            control={<Switch disabled={!automaticGameModeAllowed} checked={automaticGameModeAllowed && aiPolicy.auto_pc_clean} onCheckedChange={(auto_pc_clean) => updateAiPolicy({ auto_pc_clean })} />}
          />
          <Row
            label={t("settings.aiAutoRestore")}
            description={t("settings.aiAutoRestoreDesc")}
            control={<Switch checked={aiPolicy.auto_restore_game_mode} onCheckedChange={(auto_restore_game_mode) => updateAiPolicy({ auto_restore_game_mode })} />}
          />
          <Row
            label={t("settings.aiAutomaticSensitive")}
            description={t("settings.aiAutomaticSensitiveDesc")}
            control={<Switch checked={aiPolicy.allow_automatic_sensitive_actions} onCheckedChange={(allow_automatic_sensitive_actions) => updateAiPolicy({ allow_automatic_sensitive_actions })} />}
          />
          <Row
            label={t("settings.aiPowerPlan")}
            description={t("settings.aiPowerPlanDesc")}
            control={<Switch checked={aiPolicy.optimize_power_plan} onCheckedChange={(optimize_power_plan) => updateAiPolicy({ optimize_power_plan })} />}
          />
          <Row
            label={t("settings.aiCleanup")}
            description={t("settings.aiCleanupDesc")}
            control={<Switch checked={aiPolicy.safe_temp_cleanup} onCheckedChange={(safe_temp_cleanup) => updateAiPolicy({ safe_temp_cleanup })} />}
          />
          <Row
            label={t("settings.aiEnergyEstimation")}
            description={t("settings.aiEnergyEstimationDesc")}
            control={<Switch checked={aiPolicy.energy_estimation_enabled} onCheckedChange={(energy_estimation_enabled) => updateAiPolicy({ energy_estimation_enabled })} />}
          />
          <Row
            label={t("settings.aiThermalAnalysis")}
            description={t("settings.aiThermalAnalysisDesc")}
            control={<Switch checked={aiPolicy.thermal_analysis_enabled} onCheckedChange={(thermal_analysis_enabled) => updateAiPolicy({ thermal_analysis_enabled })} />}
          />
          <Row
            label={t("settings.aiStartup")}
            description={t("settings.aiStartupDesc")}
            control={<Switch checked={aiPolicy.manage_startup_apps} onCheckedChange={(manage_startup_apps) => updateAiPolicy({ manage_startup_apps })} />}
          />
          <Row
            label={t("settings.aiServices")}
            description={t("settings.aiServicesDesc")}
            control={<Switch checked={aiPolicy.manage_services} onCheckedChange={(manage_services) => updateAiPolicy({ manage_services, max_risk: manage_services ? "sensitive" : aiPolicy.max_risk })} />}
          />
          <Row
            label={t("settings.aiBackground")}
            description={t("settings.aiBackgroundDesc")}
            control={<Switch checked={aiPolicy.reduce_background_processes} onCheckedChange={(reduce_background_processes) => updateAiPolicy({ reduce_background_processes })} />}
          />
        </div>

        <div className="mt-3 rounded-xl border border-cyan-500/10 bg-slate-950/40 p-4">
          <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
            <div>
              <div className="text-sm font-semibold text-slate-100">{t("settings.aiRisk")}</div>
              <div className="mt-0.5 text-xs text-slate-500">{t("settings.aiRiskDesc")}</div>
            </div>
            <Select value={aiPolicy.max_risk} onValueChange={(max_risk) => updateAiPolicy({ max_risk })}>
              <SelectTrigger className="w-40 border-cyan-500/20 bg-slate-950/60 font-mono text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="safe">{t("settings.aiRiskSafe")}</SelectItem>
                <SelectItem value="sensitive">{t("settings.aiRiskSensitive")}</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>

        <div className="mt-4 rounded-xl border border-cyan-500/10 bg-slate-950/40 p-4">
          <div className="mb-3 text-sm font-semibold text-slate-100">{t("settings.aiRules")}</div>
          <div className="grid gap-3 md:grid-cols-2">
            <NumberRule label={t("settings.aiGameConfidence")} value={aiPolicy.game_min_confidence} min={0.5} max={0.98} step={0.01} onChange={(game_min_confidence) => updateAiPolicy({ game_min_confidence })} />
            <NumberRule label={t("settings.aiGameCooldown")} value={aiPolicy.game_cooldown_seconds} min={60} max={21600} step={60} onChange={(game_cooldown_seconds) => updateAiPolicy({ game_cooldown_seconds })} />
            <NumberRule label={t("settings.aiPcCleanCooldown")} value={aiPolicy.pc_clean_cooldown_seconds} min={600} max={86400} step={300} onChange={(pc_clean_cooldown_seconds) => updateAiPolicy({ pc_clean_cooldown_seconds })} />
            <NumberRule label={t("settings.aiCleanupIdle")} value={aiPolicy.cleanup_min_idle_seconds} min={60} max={43200} step={60} onChange={(cleanup_min_idle_seconds) => updateAiPolicy({ cleanup_min_idle_seconds })} />
            <NumberRule label={t("settings.aiCleanupDisk")} value={aiPolicy.cleanup_disk_threshold_percent} min={70} max={99} step={1} suffix="%" onChange={(cleanup_disk_threshold_percent) => updateAiPolicy({ cleanup_disk_threshold_percent })} />
            <NumberRule label={t("settings.aiCpuThermal")} value={aiPolicy.thermal_cpu_limit_c} min={70} max={105} step={1} suffix="C" onChange={(thermal_cpu_limit_c) => updateAiPolicy({ thermal_cpu_limit_c })} />
            <NumberRule label={t("settings.aiGpuThermal")} value={aiPolicy.thermal_gpu_limit_c} min={70} max={100} step={1} suffix="C" onChange={(thermal_gpu_limit_c) => updateAiPolicy({ thermal_gpu_limit_c })} />
            <NumberRule label={t("settings.aiBatterySaver")} value={aiPolicy.battery_saver_threshold_percent} min={5} max={50} step={1} suffix="%" onChange={(battery_saver_threshold_percent) => updateAiPolicy({ battery_saver_threshold_percent })} />
            <NumberRule label={t("settings.aiNetworkLatency")} value={aiPolicy.network_latency_threshold_ms} min={40} max={500} step={5} suffix="ms" onChange={(network_latency_threshold_ms) => updateAiPolicy({ network_latency_threshold_ms })} />
          </div>
        </div>

        <div className="mt-4 flex flex-col gap-3">
          <PolicyGroup
            icon={<Trash2 className="h-4 w-4 text-cyan-300" />}
            title={t("settings.aiCleanupGroup")}
            description={t("settings.aiCleanupGroupDesc")}
            onOpenHistory={onOpenHistory}
          >
            <NumberRule label={t("settings.aiCleanupTempAge")} value={aiPolicy.cleanup_temp_min_age_minutes} min={5} max={10080} step={5} suffix="min" onChange={(cleanup_temp_min_age_minutes) => updateAiPolicy({ cleanup_temp_min_age_minutes })} />
            <NumberRule label={t("settings.aiCleanupCacheAge")} value={aiPolicy.cleanup_cache_min_age_minutes} min={10} max={10080} step={10} suffix="min" onChange={(cleanup_cache_min_age_minutes) => updateAiPolicy({ cleanup_cache_min_age_minutes })} />
            <NumberRule label={t("settings.aiCleanupSystemAge")} value={aiPolicy.cleanup_system_min_age_minutes} min={60} max={43200} step={60} suffix="min" onChange={(cleanup_system_min_age_minutes) => updateAiPolicy({ cleanup_system_min_age_minutes })} />
          </PolicyGroup>

          <PolicyGroup
            icon={<Battery className="h-4 w-4 text-cyan-300" />}
            title={t("settings.aiEnergyGroup")}
            description={t("settings.aiEnergyGroupDesc")}
            onOpenHistory={onOpenHistory}
          >
            <NumberRule label={t("settings.aiIdleEcoThreshold")} value={aiPolicy.adaptive_idle_eco_threshold_seconds} min={60} max={10800} step={30} suffix="s" onChange={(adaptive_idle_eco_threshold_seconds) => updateAiPolicy({ adaptive_idle_eco_threshold_seconds })} />
          </PolicyGroup>
        </div>
      </section>

      <section className="glass-panel cyber-glow p-6">
        <div className="flex items-center gap-2 pb-4">
          <Eye className="h-3.5 w-3.5 text-cyan-300" />
          <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-cyan-400/80">{t("settings.accessibility")}</h2>
        </div>
        <div className="flex flex-col gap-3">
          <Row
            label={t("settings.highContrast")}
            description={t("settings.highContrastDesc")}
            control={<Switch checked={highContrast} onCheckedChange={setHighContrast} />}
          />
          <Row
            label={t("settings.reduceMotion")}
            description={t("settings.reduceMotionDesc")}
            control={<Switch checked={reduceMotion} onCheckedChange={setReduceMotion} />}
          />
        </div>
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
          <DownloadCloud className="h-3.5 w-3.5 text-cyan-300" />
          <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-cyan-400/80">{t("update.title")}</h2>
          {updater.status?.available && (
            <span className="ml-2 inline-flex items-center gap-1 rounded-md border border-cyan-400/40 bg-cyan-500/10 px-2 py-0.5 font-mono text-[9px] uppercase tracking-widest text-cyan-200">
              <span className="h-1 w-1 animate-pulse rounded-full bg-cyan-300" />
              {t("update.badgeAvailable")}
            </span>
          )}
        </div>
        <div className="flex flex-col gap-3">
          <Row
            label={t("update.currentVersion")}
            description={
              updater.status?.available && isUpdateDismissedNow(updater.status)
                ? t("update.availableTitle", { version: updater.status.version ?? "" })
                : updater.status
                  ? t("update.upToDate")
                  : t("update.neverChecked")
            }
            control={
              <button
                disabled={checkingUpdate}
                onClick={checkForUpdatesNow}
                className="inline-flex items-center gap-2 rounded-xl border border-cyan-400/40 bg-cyan-400/10 px-3 py-2 text-xs font-medium text-cyan-100 transition-all hover:border-cyan-300/60 disabled:opacity-50"
              >
                <RefreshCw className={`h-3.5 w-3.5 ${checkingUpdate ? "animate-spin" : ""}`} />
                {checkingUpdate ? t("update.checking") : t("update.checkButton")}
              </button>
            }
          />
          <div className="rounded-xl border border-cyan-500/10 bg-slate-950/40 p-4 font-mono text-[11px] uppercase tracking-widest text-slate-500">
            {updater.status?.currentVersion ?? "-"}
          </div>
        </div>
      </section>

      <section className="glass-panel cyber-glow p-6">
        <div className="flex items-center gap-2 pb-4">
          <UserIcon className="h-3.5 w-3.5 text-cyan-300" />
          <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-cyan-400/80">{t("settings.account")}</h2>
        </div>

        {user ? (
          <div className="flex flex-col gap-4 rounded-xl border border-cyan-500/15 bg-slate-950/40 p-5 md:flex-row md:items-center">
            <div className="grid h-14 w-14 place-items-center rounded-full border border-cyan-400/30 bg-cyan-500/15 text-lg font-semibold text-cyan-100">
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
            <button
              onClick={() => void onOpenAccountSettings()}
              className="inline-flex items-center gap-2 rounded-xl border border-cyan-400/40 bg-cyan-400/10 px-4 py-2.5 text-sm font-medium text-cyan-100 transition-all hover:border-cyan-300/60 hover:bg-cyan-400/15"
            >
              <ExternalLink className="h-4 w-4" />
              {t("settings.manageDevices")}
            </button>
            {!user.hasPaidPlan && (
              <button
                onClick={() => void onOpenBilling()}
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
              className="group inline-flex items-center gap-2.5 rounded-xl border border-cyan-400/40 bg-cyan-400/10 px-5 py-2.5 text-sm font-semibold text-cyan-100 transition-all duration-300 hover:border-cyan-300/60"
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
        <p className="mt-3 rounded-xl border border-cyan-500/10 bg-slate-950/40 p-4 text-xs text-slate-400">
          {t("settings.deviceRule")}
        </p>
        <div className="mt-4 flex flex-wrap gap-2">
          <ActionButton disabled={busy || !isReady} onClick={onStartAgent}>{t("settings.startAgent")}</ActionButton>
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

const DEFAULT_LOCAL_AI_POLICY: LocalAiPolicy = {
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
  cleanup_cache_min_age_minutes: 360,
  cleanup_temp_min_age_minutes: 60,
  cleanup_system_min_age_minutes: 1440,
  adaptive_idle_eco_threshold_seconds: 600,
};

function PolicyGroup({
  icon,
  title,
  description,
  onOpenHistory,
  children,
}: {
  icon: React.ReactNode;
  title: string;
  description: string;
  onOpenHistory?: () => void;
  children: React.ReactNode;
}) {
  const { t } = useI18n();
  return (
    <details className="group rounded-xl border border-cyan-500/10 bg-slate-950/40">
      <summary className="flex cursor-pointer list-none items-center justify-between gap-3 rounded-xl px-4 py-3 transition hover:bg-cyan-400/5">
        <span className="flex min-w-0 items-center gap-3">
          <span className="grid h-8 w-8 shrink-0 place-items-center rounded-lg border border-cyan-500/20 bg-cyan-400/10">
            {icon}
          </span>
          <span className="min-w-0">
            <span className="block text-sm font-semibold text-slate-100">{title}</span>
            <span className="mt-0.5 block text-xs leading-relaxed text-slate-500">{description}</span>
          </span>
        </span>
        <span className="shrink-0 font-mono text-[10px] uppercase tracking-widest text-cyan-300 group-open:hidden">
          abrir
        </span>
        <span className="hidden shrink-0 font-mono text-[10px] uppercase tracking-widest text-cyan-300 group-open:block">
          recolher
        </span>
      </summary>
      <div className="flex flex-col gap-3 border-t border-cyan-500/10 p-4">
        {children}
        {onOpenHistory && (
          <button
            type="button"
            onClick={onOpenHistory}
            className="inline-flex w-fit items-center gap-1.5 font-mono text-[10px] uppercase tracking-widest text-cyan-300 transition hover:text-cyan-200"
          >
            <History className="h-3 w-3" />
            {t("settings.aiViewRestoreHistory")}
          </button>
        )}
      </div>
    </details>
  );
}

function NumberRule({
  label,
  value,
  min,
  max,
  step,
  suffix,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  suffix?: string;
  onChange: (value: number) => void;
}) {
  return (
    <label className="flex items-center justify-between gap-3 rounded-lg border border-cyan-500/10 bg-slate-950/40 px-3 py-2">
      <span className="text-xs text-slate-400">{label}</span>
      <span className="flex items-center gap-2">
        <input
          type="number"
          value={value}
          min={min}
          max={max}
          step={step}
          onChange={(event) => {
            const nextValue = Number(event.target.value);
            if (Number.isFinite(nextValue)) onChange(nextValue);
          }}
          className="h-8 w-24 rounded-md border border-cyan-500/20 bg-slate-950/70 px-2 text-right font-mono text-xs text-slate-100 outline-none focus:border-cyan-300/60"
        />
        {suffix && <span className="w-7 font-mono text-[10px] uppercase text-slate-500">{suffix}</span>}
      </span>
    </label>
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
      className="rounded-xl border border-cyan-400/40 bg-cyan-400/10 px-4 py-2.5 text-sm font-semibold text-cyan-100 transition-all hover:border-cyan-300/60 disabled:opacity-50"
    >
      {children}
    </button>
  );
}
