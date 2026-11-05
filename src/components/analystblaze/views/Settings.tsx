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
  onOpenControls,
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
  /** Automações e componente de sistema agora ficam em Controles. */
  onOpenControls?: () => void;
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

      <section className="glass-panel p-6">
        <div className="flex items-center justify-between gap-4">
          <div>
            <div className="text-sm font-semibold text-fg">{t("controls.d3.movedTitle")}</div>
            <div className="mt-0.5 text-xs text-fg-3">{t("controls.d3.movedDesc")}</div>
          </div>
          {onOpenControls && (
            <button
              type="button"
              onClick={onOpenControls}
              className="shrink-0 text-sm font-medium text-accent-2 underline underline-offset-4 hover:text-accent"
            >
              {t("controls.d3.movedAction")}
            </button>
          )}
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

