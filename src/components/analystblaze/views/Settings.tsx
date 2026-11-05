import { AlertTriangle, Battery, Brain, Check, DownloadCloud, ExternalLink, Eye, Globe, History, LogOut, Moon, RefreshCw, Shield, Sparkles, Sun, Trash2, User as UserIcon, Wrench, Zap } from "lucide-react";
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
import { canUseAutomaticGameMode, type User } from "@/hooks/useAuth";
import { localeLabels, type Locale, useI18n } from "@/i18n";
import {
  getWeeklyAiUsage,
  installPrivilegedHelper,
  isTauriRuntime,
  listenToWeeklyAiUsage,
  restartPrivilegedHelper,
  startPrivilegedHelper,
  stopPrivilegedHelper,
  testPrivilegedHelper,
  uninstallPrivilegedHelper,
  type AgentStatus,
  type LocalAiPolicy,
  type PrivilegedHelperStatus,
  type UpdateStatus,
  type WeeklyAiUsage,
} from "@/services/tauri/agent";
import { getTelemetryQueueSize, isTelemetryEnabled, setTelemetryEnabled } from "@/services/telemetry";
import { formatAgo, formatMinutes, MISSING } from "@/lib/format";
import { privilegedHelperStatus, useHelper } from "@/features/helper/useHelper";
import { automationPolicy, saveAutomationPolicy } from "@/features/automations/useAutomationPolicy";

export function Settings({
  user,
  status,
  syncingPlan,
  onLogin,
  onLogout,
  onOpenAccountSettings,
  onOpenBilling,
  onSyncPlan,
  onOpenHistory,
}: {
  user: User | null;
  status: AgentStatus | null;
  syncingPlan: boolean;
  onLogin: () => Promise<void>;
  onLogout: () => Promise<void>;
  onOpenAccountSettings: () => Promise<void>;
  onOpenBilling: () => Promise<void>;
  onSyncPlan: () => Promise<unknown>;
  onOpenHistory?: () => void;
}) {
  const { theme, setTheme } = useTheme();
  const { t, locale, setLocale } = useI18n();
  const track = useTelemetry("settings");
  const updater = useUpdater();
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [updateCheckPopup, setUpdateCheckPopup] = useState<"available" | "up_to_date" | null>(null);
  const checkForUpdatesNow = () => {
    setCheckingUpdate(true);
    updater
      .check()
      .then((next) => {
        track("update_check_requested", { available: next.available });
        setUpdateCheckPopup(next.available ? "available" : "up_to_date");
      })
      .catch(() => undefined)
      .finally(() => setCheckingUpdate(false));
  };
  const runtimeAvailable = isTauriRuntime();
  // Estado do componente de sistema: a mesma leitura do app inteiro (features/helper).
  const helperStatus = useHelper({ auto: runtimeAvailable }).status;
  const setHelperStatus = (value: PrivilegedHelperStatus | null) => privilegedHelperStatus.set(value ?? undefined);
  const [helperBusy, setHelperBusy] = useState(false);
  const [helperMessage, setHelperMessage] = useState<string | null>(null);
  const [helperError, setHelperError] = useState<string | null>(null);

  const helperStateLabel = !helperStatus
    ? t("settings.helperStateUnknown")
    : !helperStatus.installed
      ? t("settings.helperStateNotInstalled")
      : !helperStatus.running
        ? t("settings.helperStateStopped")
        : helperStatus.available
          ? t("settings.helperStateRunning")
          : t("settings.helperStateRunningAttention");
  const helperStateStyle = !helperStatus?.installed
    ? "border-danger/30 bg-danger/10 text-danger-soft"
    : !helperStatus.running
      ? "border-warn/30 bg-warn/10 text-warn-soft"
      : helperStatus.available
        ? "border-ok/30 bg-ok/10 text-ok-soft"
        : "border-warn/30 bg-warn/10 text-warn-soft";

  const runHelperAction = async (action: () => Promise<PrivilegedHelperStatus>, successMessage: string) => {
    setHelperBusy(true);
    setHelperMessage(null);
    setHelperError(null);
    try {
      const nextStatus = await action();
      setHelperStatus(nextStatus);
      setHelperMessage(successMessage);
    } catch (error) {
      setHelperError(error instanceof Error ? error.message : String(error));
    } finally {
      setHelperBusy(false);
    }
  };

  const runHelperTest = async () => {
    setHelperBusy(true);
    setHelperMessage(null);
    setHelperError(null);
    try {
      const handshake = await testPrivilegedHelper();
      setHelperStatus(await privilegedHelperStatus.refresh());
      if (handshake.ok) {
        setHelperMessage(handshake.message);
      } else {
        setHelperError(handshake.message);
      }
      track("privileged_helper_tested", { ok: handshake.ok, latencyMs: handshake.latencyMs });
    } catch (error) {
      setHelperError(error instanceof Error ? error.message : String(error));
    } finally {
      setHelperBusy(false);
    }
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
  const [weeklyAiUsage, setWeeklyAiUsage] = useState<WeeklyAiUsage | null>(null);

  useEffect(() => {
    getWeeklyAiUsage().then(setWeeklyAiUsage).catch(() => undefined);
    let dispose: (() => void) | undefined;
    listenToWeeklyAiUsage(setWeeklyAiUsage).then((unlisten) => {
      dispose = unlisten;
    });
    return () => dispose?.();
  }, []);

  useEffect(() => {
    automationPolicy
      .refresh()
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
    saveAutomationPolicy(nextPolicy)
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
        <div className="font-mono text-[11px] uppercase tracking-[0.3em] text-accent/70">
          {t("settings.eyebrow")}
        </div>
        <h1 className="text-[36px] font-semibold tracking-tight text-fg">
          {t("settings.title")}
        </h1>
        <p className="max-w-3xl text-sm text-fg-3">{t("settings.description")}</p>
      </header>

      {!onboardingDone && (
        <section className="glass-panel p-6"role="region"aria-label={t("settings.onboardingTitle")}>
          <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
            <div className="flex items-start gap-3">
              <div className="grid h-10 w-10 shrink-0 place-items-center rounded-xl border border-accent/30 bg-accent-strong/10">
                <Check className="h-4 w-4 text-accent" />
              </div>
              <div>
                <h2 className="text-sm font-semibold text-fg">{t("settings.onboardingTitle")}</h2>
                <p className="mt-1 max-w-2xl text-xs leading-relaxed text-fg-3">
                  {t("settings.onboardingDesc")}
                </p>
                <div className="mt-3 flex flex-wrap gap-1.5">
                  {["login", "telemetry", "restore", "privacy"].map((step) => (
                    <span key={step} className="rounded-md border border-line bg-bg/60 px-2 py-0.5 font-mono text-[9px] uppercase tracking-widest text-fg-3">
                      {t(`settings.onboardingSteps.${step}`)}
                    </span>
                  ))}
                </div>
              </div>
            </div>
            <button
              onClick={finishOnboarding}
              className="rounded-xl border border-accent/40 bg-accent/10 px-4 py-2.5 text-sm font-medium text-accent-soft transition-all hover:border-accent/60 hover:bg-accent/15"
            >
              {t("settings.onboardingDone")}
            </button>
          </div>
        </section>
      )}

      <section className="glass-panel p-6">
        <div className="flex items-center gap-2 pb-4">
          {dark ? <Moon className="h-3.5 w-3.5 text-accent" /> : <Sun className="h-3.5 w-3.5 text-accent" />}
          <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-accent/80">{t("settings.appearance")}</h2>
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
                <SelectTrigger className="w-40 border-line bg-bg/60 font-mono text-xs">
                  <Globe className="mr-2 h-3.5 w-3.5 text-accent" />
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
          <Row
            label={t("settings.autostart")}
            description={t("settings.autostartDesc")}
            control={<Switch checked={aiPolicy.autostart_enabled} onCheckedChange={(autostart_enabled) => updateAiPolicy({ autostart_enabled })} />}
          />
          <Row
            label={t("settings.startMinimized")}
            description={t("settings.startMinimizedDesc")}
            control={<Switch checked={aiPolicy.start_minimized} onCheckedChange={(start_minimized) => updateAiPolicy({ start_minimized })} />}
          />
          <Row
            label={t("settings.telemetryDiagnostics")}
            description={t("settings.telemetryDiagnosticsDesc")}
            control={
              <span className="shrink-0 rounded-md border border-accent/30 bg-accent/10 px-2 py-1 font-mono text-[10px] uppercase tracking-widest text-accent-soft">
                {t("settings.telemetryDiagnosticsAlwaysOn")}
              </span>
            }
          />
        </div>
      </section>

      <section className={`glass-panel p-6 transition-all ${adaptive ? "ring-1 ring-accent/40": ""}`}>
        <div className="flex items-center gap-2 pb-4">
          <Brain className="h-3.5 w-3.5 text-accent" />
          <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-accent/80">{t("settings.adaptive")}</h2>
          {adaptive && (
            <span className="ml-2 inline-flex items-center gap-1 rounded-md border border-accent/40 bg-accent-strong/10 px-2 py-0.5 font-mono text-[9px] uppercase tracking-widest text-accent-soft">
              <span className="h-1 w-1 animate-pulse rounded-full bg-accent" />
              {t("settings.adaptiveActive")}
            </span>
          )}
        </div>

        <div className="flex items-start justify-between gap-4 rounded-xl border border-line bg-bg/40 p-5">
          <div className="flex items-start gap-3">
            <div className="grid h-10 w-10 shrink-0 place-items-center rounded-xl border border-accent/30 bg-accent-strong/10">
              <Sparkles className="h-4 w-4 text-accent" />
            </div>
            <div>
              <div className="text-sm font-semibold text-fg">{t("settings.adaptiveDecision")}</div>
              <div className="mt-0.5 max-w-md text-xs text-fg-4">
                {t("settings.adaptiveDesc")}
              </div>
              <div className="mt-3 flex flex-wrap gap-1.5">
                {["energy", "processes", "network", "fans"].map((tag) => (
                  <span key={tag} className="rounded-md border border-line bg-bg/60 px-2 py-0.5 font-mono text-[9px] uppercase tracking-widest text-fg-3">
                    {t(`settings.tags.${tag}`)}
                  </span>
                ))}
              </div>
            </div>
          </div>
          <Switch checked={aiPolicy.enabled} onCheckedChange={(enabled) => updateAiPolicy({ enabled, agent_mode: enabled ? "automatic" : "manual" })} />
        </div>

        {adaptive && (
          <div className="mt-3 flex items-center gap-2 rounded-lg border border-warn/20 bg-warn/5 px-3 py-2 text-[11px] text-warn-soft/80">
            <Zap className="h-3 w-3" />
            {t("settings.adaptiveWarning")}
          </div>
        )}
        {!automaticGameModeAllowed && (
          <div className="mt-3 flex items-center gap-2 rounded-lg border border-line bg-accent/5 px-3 py-2 text-[11px] text-accent-soft/80">
            <Shield className="h-3 w-3" />
            {t("settings.aiAutomationPlanNotice")}
          </div>
        )}
        {weeklyAiUsage && weeklyAiUsage.limitSeconds != null && (
          <div className={`mt-3 flex items-center gap-2 rounded-lg border px-3 py-2 text-[11px] ${weeklyAiUsage.limitReached ? "border-warn/20 bg-warn/5 text-warn-soft/80" : "border-line bg-accent/5 text-accent-soft/80"}`}>
            <Zap className="h-3 w-3" />
            {weeklyAiUsage.limitReached
              ? "Limite semanal de automacao atingido. Volta na próxima semana, ou assine um plano pago para automacao continua."
              : `Automacao: ${formatMinutes(weeklyAiUsage.remainingSeconds ?? 0)} restantes esta semana (plano gratuito).`}
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
            label={t("settings.aiAutoBestDns")}
            description={t("settings.aiAutoBestDnsDesc")}
            control={<Switch checked={aiPolicy.auto_best_dns} onCheckedChange={(auto_best_dns) => updateAiPolicy({ auto_best_dns })} />}
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

        <div className="mt-3 rounded-xl border border-line bg-bg/40 p-4">
          <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
            <div>
              <div className="text-sm font-semibold text-fg">{t("settings.aiRisk")}</div>
              <div className="mt-0.5 text-xs text-fg-4">{t("settings.aiRiskDesc")}</div>
            </div>
            <Select value={aiPolicy.max_risk} onValueChange={(max_risk) => updateAiPolicy({ max_risk })}>
              <SelectTrigger className="w-40 border-line bg-bg/60 font-mono text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="safe">{t("settings.aiRiskSafe")}</SelectItem>
                <SelectItem value="sensitive">{t("settings.aiRiskSensitive")}</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>

        <div className="mt-4 flex flex-col gap-3">
          <PolicyGroup
            icon={<Trash2 className="h-4 w-4 text-accent" />}
            title={t("settings.aiCleanupGroup")}
            description={t("settings.aiCleanupGroupDesc")}
            onOpenHistory={onOpenHistory}
          >
            <NumberRule label={t("settings.aiCleanupTempAge")} value={aiPolicy.cleanup_temp_min_age_minutes} min={5} max={10080} step={5} suffix="min" onChange={(cleanup_temp_min_age_minutes) => updateAiPolicy({ cleanup_temp_min_age_minutes })} />
            <NumberRule label={t("settings.aiCleanupCacheAge")} value={aiPolicy.cleanup_cache_min_age_minutes} min={10} max={10080} step={10} suffix="min" onChange={(cleanup_cache_min_age_minutes) => updateAiPolicy({ cleanup_cache_min_age_minutes })} />
            <NumberRule label={t("settings.aiCleanupSystemAge")} value={aiPolicy.cleanup_system_min_age_minutes} min={60} max={43200} step={60} suffix="min" onChange={(cleanup_system_min_age_minutes) => updateAiPolicy({ cleanup_system_min_age_minutes })} />
          </PolicyGroup>

          <PolicyGroup
            icon={<Battery className="h-4 w-4 text-accent" />}
            title={t("settings.aiEnergyGroup")}
            description={t("settings.aiEnergyGroupDesc")}
            onOpenHistory={onOpenHistory}
          >
            <NumberRule label={t("settings.aiIdleEcoThreshold")} value={aiPolicy.adaptive_idle_eco_threshold_seconds} min={60} max={10800} step={30} suffix="s" onChange={(adaptive_idle_eco_threshold_seconds) => updateAiPolicy({ adaptive_idle_eco_threshold_seconds })} />
          </PolicyGroup>
        </div>
      </section>

      <section className="glass-panel p-6">
        <div className="flex items-center gap-2 pb-4">
          <Eye className="h-3.5 w-3.5 text-accent" />
          <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-accent/80">{t("settings.accessibility")}</h2>
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

      <section className="glass-panel p-6">
        <div className="flex items-center gap-2 pb-4">
          <Shield className="h-3.5 w-3.5 text-accent" />
          <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-accent/80">{t("settings.privacy")}</h2>
        </div>
        <Row
          label={t("settings.uiTelemetry")}
          description={t("settings.uiTelemetryDesc")}
          control={<Switch checked={telem} onCheckedChange={setTelem} />}
        />
        <div className="mt-3 rounded-xl border border-line bg-bg/40 p-4 font-mono text-[11px] uppercase tracking-widest text-fg-4">
          {t("settings.telemetryBuffer")}: {queueSize}
        </div>
      </section>

      <section className="glass-panel p-6">
        <div className="flex items-center gap-2 pb-4">
          <DownloadCloud className="h-3.5 w-3.5 text-accent" />
          <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-accent/80">{t("update.title")}</h2>
          {updater.status?.available && (
            <span className="ml-2 inline-flex items-center gap-1 rounded-md border border-accent/40 bg-accent-strong/10 px-2 py-0.5 font-mono text-[9px] uppercase tracking-widest text-accent-soft">
              <span className="h-1 w-1 animate-pulse rounded-full bg-accent" />
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
                className="inline-flex items-center gap-2 rounded-xl border border-accent/40 bg-accent/10 px-3 py-2 text-xs font-medium text-accent-soft transition-all hover:border-accent/60 disabled:opacity-50"
              >
                <RefreshCw className={`h-3.5 w-3.5 ${checkingUpdate ? "animate-spin" : ""}`} />
                {checkingUpdate ? t("update.checking") : t("update.checkButton")}
              </button>
            }
          />
          <div className="rounded-xl border border-line bg-bg/40 p-4 font-mono text-[11px] uppercase tracking-widest text-fg-4">
            {updater.status?.currentVersion ?? "-"}
          </div>
        </div>
      </section>

      <section className="glass-panel p-6">
        <div className="flex items-center gap-2 pb-4">
          <Wrench className="h-3.5 w-3.5 text-accent" />
          <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-accent/80">{t("settings.privilegedHelper")}</h2>
        </div>
        <p className="text-xs text-fg-4">{t("settings.privilegedHelperDesc")}</p>

        <div className="mt-4 rounded-xl border border-line bg-bg/40 p-4">
          <p className="text-xs text-fg-4">
            {helperStatus?.message ?? t("settings.helperUnknown")}
          </p>
          <div className="mt-3 flex items-center gap-2 text-xs">
            <span className={`inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 font-medium ${helperStateStyle}`}>
              <span className="h-1.5 w-1.5 rounded-full bg-current" />
              {helperStateLabel}
            </span>
          </div>
          <div className="mt-3 grid gap-2 text-xs text-fg-3 sm:grid-cols-3">
            <span className="rounded-lg border border-line bg-bg/50 px-3 py-2">
              {t("settings.helperInstalled")}: {helperStatus?.installed ? t("common.yes") : t("common.no")}
            </span>
            <span className="rounded-lg border border-line bg-bg/50 px-3 py-2">
              {t("settings.helperRunning")}: {helperStatus?.running ? t("common.yes") : t("common.no")}
            </span>
            <span className="rounded-lg border border-line bg-bg/50 px-3 py-2">
              {t("settings.helperVersion")}: {helperStatus?.version ?? MISSING}
            </span>
          </div>

          {helperMessage && (
            <div className="mt-3 flex items-start gap-2 rounded-xl border border-line bg-accent/10 px-4 py-3 text-sm text-accent-soft">
              {helperMessage}
            </div>
          )}
          {helperError && (
            <div className="mt-3 flex items-start gap-2 rounded-xl border border-danger/25 bg-danger/10 px-4 py-3 text-sm text-danger-soft">
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
              {helperError}
            </div>
          )}

          <div className="mt-4 flex flex-wrap gap-2">
            <button
              disabled={helperBusy || !runtimeAvailable || helperStatus?.available === true || helperStatus?.canRequestUac === false}
              onClick={() => void runHelperAction(installPrivilegedHelper, helperStatus?.installed ? t("settings.helperRepaired") : t("settings.helperInstalledMsg"))}
              title={helperStatus?.canRequestUac === false ? t("settings.helperNeedsPerMachine") : (helperStatus?.installed ? t("settings.helperRepairHint") : t("settings.helperInstallHint"))}
              className="rounded-lg border border-ok/30 bg-ok/10 px-3 py-2 text-xs font-medium text-ok-soft transition hover:bg-ok/15 disabled:opacity-50"
            >
              {helperStatus?.installed ? t("settings.helperInstallRepair") : t("settings.helperInstallButton")}
            </button>
            <button
              disabled={helperBusy || !runtimeAvailable || !helperStatus?.installed || helperStatus?.running === true}
              onClick={() => void runHelperAction(startPrivilegedHelper, t("settings.helperStarted"))}
              className="rounded-lg border border-ok/30 bg-ok/10 px-3 py-2 text-xs font-medium text-ok-soft transition hover:bg-ok/15 disabled:opacity-50"
            >
              {t("settings.helperStart")}
            </button>
            <button
              disabled={helperBusy || !runtimeAvailable || !helperStatus?.installed || helperStatus?.running === false}
              onClick={() => void runHelperAction(stopPrivilegedHelper, t("settings.helperStopped"))}
              className="rounded-lg border border-warn/30 bg-warn/10 px-3 py-2 text-xs font-medium text-warn-soft transition hover:bg-warn/15 disabled:opacity-50"
            >
              {t("settings.helperStop")}
            </button>
            <button
              disabled={helperBusy || !runtimeAvailable || !helperStatus?.installed}
              onClick={() => void runHelperAction(restartPrivilegedHelper, t("settings.helperRestarted"))}
              className="rounded-lg border border-accent/30 bg-accent/10 px-3 py-2 text-xs font-medium text-accent-soft transition hover:bg-accent/15 disabled:opacity-50"
            >
              {t("settings.helperRestart")}
            </button>
            <button
              disabled={helperBusy || !runtimeAvailable || !helperStatus?.installed}
              onClick={() => void runHelperTest()}
              title={t("settings.helperTestHint")}
              className="rounded-lg border border-accent-2/30 bg-accent-2/10 px-3 py-2 text-xs font-medium text-accent-2 transition hover:bg-accent-2/15 disabled:opacity-50"
            >
              {t("settings.helperTest")}
            </button>
            <button
              disabled={helperBusy || !runtimeAvailable || !helperStatus?.installed}
              onClick={() => void runHelperAction(uninstallPrivilegedHelper, t("settings.helperRemoved"))}
              className="rounded-lg border border-danger/30 bg-danger/10 px-3 py-2 text-xs font-medium text-danger-soft transition hover:bg-danger/15 disabled:opacity-50"
            >
              {t("settings.helperRemove")}
            </button>
          </div>
        </div>
      </section>

      <section className="glass-panel p-6">
        <div className="flex items-center gap-2 pb-4">
          <UserIcon className="h-3.5 w-3.5 text-accent" />
          <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-accent/80">{t("settings.account")}</h2>
        </div>

        {user ? (
          <div className="flex flex-col gap-4 rounded-xl border border-line bg-bg/40 p-5 md:flex-row md:items-center">
            <div className="grid h-14 w-14 place-items-center rounded-full border border-accent/30 bg-accent-strong/15 text-lg font-semibold text-accent-soft">
              {user.name.slice(0, 1).toUpperCase()}
            </div>
            <div className="flex-1">
              <div className="text-base font-semibold text-fg">{user.name}</div>
              <div className="mt-0.5 font-mono text-[10px] uppercase tracking-widest text-accent">
                {t("sidebar.currentPlan")}: {planLabel(user.plan)}
              </div>
              <div className="mt-0.5 font-mono text-[11px] text-fg-4">
                {t("sidebar.session")} - {user.sessionId.slice(0, 8)}...{user.sessionId.slice(-4)}
              </div>
              <div className="mt-2 inline-flex items-center gap-1.5 rounded-md border border-ok/30 bg-ok/10 px-2 py-0.5 font-mono text-[10px] uppercase tracking-widest text-ok">
                <span className="h-1 w-1 rounded-full bg-ok" />
                {isReady ? t("common.online") : t("common.pending")}
              </div>
              <div className="mt-2 flex items-center gap-2">
                <span
                  className={`inline-flex items-center gap-1.5 rounded-md border px-2 py-0.5 font-mono text-[10px] uppercase tracking-widest ${
                    status?.plan_sync_error
                      ? "border-warn/30 bg-warn/10 text-warn"
                      : "border-line bg-bg/60 text-fg-3"
                  }`}
                >
                  {status?.plan_sync_error
                    ? t("settings.planSyncFailed", { category: status.plan_sync_error })
                    : status?.plan_synced_at
                      ? t("settings.planSyncOk", { time: formatAgo(status.plan_synced_at) })
                      : t("settings.planSyncNever")}
                </span>
                <button
                  disabled={syncingPlan}
                  onClick={() => void onSyncPlan().catch(() => undefined)}
                  className="inline-flex items-center gap-1.5 rounded-md border border-line bg-bg/60 px-2 py-0.5 font-mono text-[10px] uppercase tracking-widest text-accent transition-all hover:border-accent/60 disabled:opacity-50"
                >
                  <RefreshCw className={`h-3 w-3 ${syncingPlan ? "animate-spin" : ""}`} />
                  {syncingPlan ? t("settings.planSyncButtonBusy") : t("settings.planSyncButton")}
                </button>
              </div>
            </div>
            <button
              onClick={() => void onOpenAccountSettings()}
              className="inline-flex items-center gap-2 rounded-xl border border-accent/40 bg-accent/10 px-4 py-2.5 text-sm font-medium text-accent-soft transition-all hover:border-accent/60 hover:bg-accent/15"
            >
              <ExternalLink className="h-4 w-4" />
              {t("settings.manageDevices")}
            </button>
            {!user.hasPaidPlan && (
              <button
                onClick={() => void onOpenBilling()}
                className="inline-flex items-center gap-2 rounded-xl border border-accent/40 bg-accent/10 px-4 py-2.5 text-sm font-medium text-accent-soft transition-all hover:border-accent/60 hover:bg-accent/15"
              >
                <ExternalLink className="h-4 w-4" />
                {t("sidebar.becomePro")}
              </button>
            )}
            <button
              onClick={() => void onLogout()}
              className="inline-flex items-center gap-2 rounded-xl border border-danger-strong/30 bg-danger-strong/10 px-4 py-2.5 text-sm font-medium text-danger transition-all hover:bg-danger-strong/20 hover:border-danger/50"
            >
              <LogOut className="h-4 w-4" />
              {t("common.logout")}
            </button>
          </div>
        ) : (
          <div className="flex flex-col items-start gap-4 rounded-xl border border-dashed border-line bg-bg/30 p-5">
            <p className="text-sm text-fg-3">
              {t("settings.loggedOut")}
            </p>
            <button
              onClick={() => void onLogin()}
              className="group inline-flex items-center gap-2.5 rounded-xl border border-accent/40 bg-accent/10 px-5 py-2.5 text-sm font-semibold text-accent-soft transition-all duration-300 hover:border-accent/60"
            >
              {t("settings.webLogin")}
              <ExternalLink className="h-4 w-4 transition-transform group-hover:translate-x-0.5 group-hover:-translate-y-0.5" />
            </button>
          </div>
        )}
      </section>

      {/* D3: "Agente Desktop" debug panel removed from Settings (status tiles, raw HW ID,
          device-rule note, "Iniciar agente" / "Coletar amostra" buttons). auth.start and
          auth.collectSample are untouched and still wired into Dashboard/Telemetry. */}
      {updateCheckPopup && (
        <UpdateCheckPopup
          result={updateCheckPopup}
          status={updater.status}
          busy={Boolean(updater.status?.installing)}
          onUpdateNow={() => {
            void updater.apply();
            setUpdateCheckPopup(null);
          }}
          onClose={() => setUpdateCheckPopup(null)}
        />
      )}
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
    <div className="flex items-center justify-between gap-4 rounded-xl border border-line bg-bg/40 p-4">
      <div>
        <div className="text-sm font-medium text-fg">{label}</div>
        <div className="mt-0.5 text-xs text-fg-4">{description}</div>
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
  auto_best_dns: false,
  dns_optimization_cooldown_seconds: 21600,
  dns_improvement_threshold_ms: 15,
  cleanup_cache_min_age_minutes: 360,
  cleanup_temp_min_age_minutes: 60,
  cleanup_system_min_age_minutes: 1440,
  adaptive_idle_eco_threshold_seconds: 600,
  autostart_enabled: true,
  start_minimized: true,
  telemetry_diagnostics_enabled: false,
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
    <details className="group rounded-xl border border-line bg-bg/40">
      <summary className="flex cursor-pointer list-none items-center justify-between gap-3 rounded-xl px-4 py-3 transition hover:bg-accent/5">
        <span className="flex min-w-0 items-center gap-3">
          <span className="grid h-8 w-8 shrink-0 place-items-center rounded-lg border border-line bg-accent/10">
            {icon}
          </span>
          <span className="min-w-0">
            <span className="block text-sm font-semibold text-fg">{title}</span>
            <span className="mt-0.5 block text-xs leading-relaxed text-fg-4">{description}</span>
          </span>
        </span>
        <span className="shrink-0 font-mono text-[10px] uppercase tracking-widest text-accent group-open:hidden">
          abrir
        </span>
        <span className="hidden shrink-0 font-mono text-[10px] uppercase tracking-widest text-accent group-open:block">
          recolher
        </span>
      </summary>
      <div className="flex flex-col gap-3 border-t border-line p-4">
        {children}
        {onOpenHistory && (
          <button
            type="button"
            onClick={onOpenHistory}
            className="inline-flex w-fit items-center gap-1.5 font-mono text-[10px] uppercase tracking-widest text-accent transition hover:text-accent-soft"
          >
            <History className="h-3 w-3" />
            {t("settings.aiViewRestoreHistory")}
          </button>
        )}
      </div>
    </details>
  );
}

function UpdateCheckPopup({
  result,
  status,
  busy,
  onUpdateNow,
  onClose,
}: {
  result: "available" | "up_to_date";
  status: UpdateStatus | null;
  busy: boolean;
  onUpdateNow: () => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const mandatory = Boolean(status?.mandatory);

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-bg/75 px-4">
      <section
        role="dialog"
        aria-modal="true"
        aria-label={t("update.title")}
        className="w-full max-w-md rounded-2xl border border-line bg-bg p-6"
      >
        <div className="font-mono text-[10px] uppercase tracking-[0.25em] text-accent/80">
          {t("update.eyebrow")}
        </div>
        {result === "available" ? (
          <>
            <h2 className="mt-2 text-xl font-semibold text-fg">
              {mandatory
                ? t("update.availableTitleMandatory", { version: status?.version ?? "" })
                : t("update.availableTitle", { version: status?.version ?? "" })}
            </h2>
            <p className="mt-2 text-sm leading-relaxed text-fg-3">
              {status?.notes?.trim() || t("update.notesFallback")}
            </p>
            {mandatory && (
              <p className="mt-2 text-xs font-medium text-danger-soft">{t("update.mandatoryNotice")}</p>
            )}
            <div className="mt-6 flex justify-end gap-2">
              {!mandatory && (
                <button
                  onClick={onClose}
                  className="rounded-xl border border-fg-4/60 px-4 py-2 text-sm font-medium text-fg-2 transition hover:border-fg-3/70"
                >
                  {t("update.later")}
                </button>
              )}
              <button
                disabled={busy}
                onClick={onUpdateNow}
                className="rounded-xl border border-accent/50 bg-accent/10 px-4 py-2 text-sm font-semibold text-accent-soft transition hover:bg-accent/15 disabled:opacity-50"
              >
                {busy ? t("update.installing") : t("update.updateNow")}
              </button>
            </div>
          </>
        ) : (
          <>
            <h2 className="mt-2 text-xl font-semibold text-fg">{t("update.upToDate")}</h2>
            <p className="mt-2 text-sm leading-relaxed text-fg-3">
              {t("update.currentVersion")}: {status?.currentVersion ?? "-"}
            </p>
            <div className="mt-6 flex justify-end">
              <button
                onClick={onClose}
                className="rounded-xl border border-accent/50 bg-accent/10 px-4 py-2 text-sm font-semibold text-accent-soft transition hover:bg-accent/15"
              >
                {t("common.close")}
              </button>
            </div>
          </>
        )}
      </section>
    </div>
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
    <label className="flex items-center justify-between gap-3 rounded-lg border border-line bg-bg/40 px-3 py-2">
      <span className="text-xs text-fg-3">{label}</span>
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
          className="h-8 w-24 rounded-md border border-line bg-bg/70 px-2 text-right font-mono text-xs text-fg outline-none focus:border-accent/60"
        />
        {suffix && <span className="w-7 font-mono text-[10px] uppercase text-fg-4">{suffix}</span>}
      </span>
    </label>
  );
}

