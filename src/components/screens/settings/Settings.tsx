import { useEffect, useRef, useState, type ReactNode } from "react";
import { ExternalLink, LogOut, Monitor, RefreshCw, Trash2 } from "lucide-react";
import { ConfirmInline } from "@/components/ui/confirm-inline";
import { removeDevice, useAccountDevices } from "@/features/account/useDevices";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { Segmented } from "@/components/ui/segmented";
import { StatusPill } from "@/components/ui/status-pill";
import { SummaryRow } from "@/components/ui/summary-row";
import { Switch } from "@/components/ui/switch";
import { toast } from "@/hooks/use-toast";
import { useTelemetry } from "@/hooks/useTelemetry";
import { useTheme, type Theme } from "@/hooks/useTheme";
import { useUpdater } from "@/hooks/useUpdater";
import type { User } from "@/hooks/useAuth";
import { localeLabels, type Locale, useI18n } from "@/i18n";
import { formatAgo, formatMinutes } from "@/lib/format";
import type { AgentStatus, LocalAiPolicy, UpdateStatus } from "@/services/tauri/agent";
import { isTelemetryEnabled, setTelemetryEnabled } from "@/services/telemetry";
import { useUiTelemetryQueue } from "@/features/telemetry/useUiTelemetryQueue";
import { useHelper } from "@/features/helper/useHelper";
import { ensureNotificationPermission } from "@/features/notifications/useAppNotifications";
import {
  automationPolicy,
  saveAutomationPolicy,
  useAutomationPolicy,
  useWeeklyAutomationUsage,
} from "@/features/automations/useAutomationPolicy";
import { errorText } from "@/features/shared/agentResource";

const ONBOARDING_KEY = "analystblaze.onboarding.done";
const HIGH_CONTRAST_KEY = "analystblaze.accessibility.highContrast";
const REDUCE_MOTION_KEY = "analystblaze.accessibility.reduceMotion";

type SettingsProps = {
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
  /** Controles › Avançado, onde o componente de sistema mora. */
  onOpenSystemComponent?: () => void;
};

function readFlag(key: string): boolean {
  try {
    return localStorage.getItem(key) === "1";
  } catch {
    return false;
  }
}

function writeFlag(key: string, value: boolean) {
  try {
    localStorage.setItem(key, value ? "1" : "0");
  } catch {
    // Preferência local: melhor esforço.
  }
}

/**
 * Configurações D3 (board Tela-Configuracoes): só preferências do app.
 * O componente de sistema e as automações moram em Controles (doc 13 / 15 §6);
 * aqui fica só a linha que leva até lá.
 */
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
  onOpenSystemComponent,
}: SettingsProps) {
  const { t } = useI18n();
  const track = useTelemetry("settings");
  const [onboardingDone, setOnboardingDone] = useState(() => readFlag(ONBOARDING_KEY));

  const finishOnboarding = () => {
    setOnboardingDone(true);
    writeFlag(ONBOARDING_KEY, true);
    track("onboarding_dismissed");
  };

  return (
    <div className="flex flex-col gap-5">
      <h2 className="sr-only">{t("settingsD3.title")}</h2>

      {!onboardingDone && (
        <Card label={t("settingsD3.onboarding.label")} role="region" aria-label={t("settingsD3.onboarding.label")}>
          <div className="flex flex-wrap items-center justify-between gap-4">
            <div className="min-w-0 flex-1">
              <div className="text-[15px] font-semibold text-fg">{t("settingsD3.onboarding.title")}</div>
              <p className="mt-0.5 max-w-3xl text-[13px] leading-snug text-fg-3">{t("settingsD3.onboarding.body")}</p>
            </div>
            <Button size="sm" variant="outline" onClick={finishOnboarding}>
              {t("settingsD3.onboarding.done")}
            </Button>
          </div>
        </Card>
      )}

      <div className="grid grid-cols-1 items-start gap-5 lg:grid-cols-2">
        <div className="flex flex-col gap-5">
          <AppearanceCard track={track} />
          <PrivacyCard track={track} onOpenAccountSettings={onOpenAccountSettings} />
          <MovedCard onOpenControls={onOpenControls} onOpenSystemComponent={onOpenSystemComponent} />
        </div>
        <div className="flex flex-col gap-5">
          <NotificationsCard onOpenHistory={onOpenHistory} />
          <AccountCard
            user={user}
            status={status}
            syncingPlan={syncingPlan}
            onLogin={onLogin}
            onLogout={onLogout}
            onOpenAccountSettings={onOpenAccountSettings}
            onOpenBilling={onOpenBilling}
            onSyncPlan={onSyncPlan}
          />
          <UpdatesCard track={track} />
        </div>
      </div>
    </div>
  );
}

type Track = ReturnType<typeof useTelemetry>;

function Foot({ children }: { children: ReactNode }) {
  return <p className="mt-3 text-xs leading-relaxed text-fg-3">{children}</p>;
}

/* ------------------------------------------------------------------ */
/* Aparência                                                           */
/* ------------------------------------------------------------------ */

function AppearanceCard({ track }: { track: Track }) {
  const { t, locale, setLocale } = useI18n();
  const { theme, setTheme } = useTheme();
  const [highContrast, setHighContrast] = useState(() => readFlag(HIGH_CONTRAST_KEY));
  const [reduceMotion, setReduceMotion] = useState(() => readFlag(REDUCE_MOTION_KEY));

  useEffect(() => {
    document.documentElement.classList.toggle("ab-high-contrast", highContrast);
    writeFlag(HIGH_CONTRAST_KEY, highContrast);
  }, [highContrast]);

  useEffect(() => {
    document.documentElement.classList.toggle("ab-reduce-motion", reduceMotion);
    writeFlag(REDUCE_MOTION_KEY, reduceMotion);
  }, [reduceMotion]);

  return (
    <Card label={t("settingsD3.appearance.label")}>
      <SummaryRow
        title={t("settingsD3.appearance.theme")}
        summary={theme === "system" ? t("settingsD3.appearance.themeSystemHint") : t("settingsD3.appearance.themeHint")}
        action={
          <Segmented<Theme>
            aria-label={t("settingsD3.appearance.theme")}
            value={theme}
            onChange={(next) => {
              setTheme(next);
              track("theme_changed", { theme: next });
            }}
            options={[
              { value: "dark", label: t("settingsD3.appearance.dark") },
              { value: "light", label: t("settingsD3.appearance.light") },
              { value: "system", label: t("settingsD3.appearance.system") },
            ]}
          />
        }
      />
      <SummaryRow
        title={t("settingsD3.appearance.language")}
        action={
          <Segmented<Locale>
            aria-label={t("settingsD3.appearance.language")}
            value={locale}
            onChange={(next) => {
              setLocale(next);
              track("locale_changed", { locale: next });
            }}
            options={(Object.entries(localeLabels) as [Locale, string][]).map(([value, label]) => ({ value, label }))}
          />
        }
      />
      <SummaryRow
        title={t("settingsD3.appearance.reduceMotion")}
        summary={t("settingsD3.appearance.reduceMotionHint")}
        action={
          <Switch
            aria-label={t("settingsD3.appearance.reduceMotion")}
            checked={reduceMotion}
            onCheckedChange={setReduceMotion}
          />
        }
      />
      <SummaryRow
        title={t("settingsD3.appearance.highContrast")}
        summary={t("settingsD3.appearance.highContrastHint")}
        action={
          <Switch
            aria-label={t("settingsD3.appearance.highContrast")}
            checked={highContrast}
            onCheckedChange={setHighContrast}
          />
        }
      />
    </Card>
  );
}

/* ------------------------------------------------------------------ */
/* Notificações                                                        */
/* ------------------------------------------------------------------ */

type PermissionState = "granted" | "denied" | "default" | "unsupported";

function readNotificationPermission(): PermissionState {
  if (typeof Notification === "undefined") return "unsupported";
  return Notification.permission;
}

function NotificationsCard({ onOpenHistory }: { onOpenHistory?: () => void }) {
  const { t } = useI18n();
  const [permission, setPermission] = useState<PermissionState>(readNotificationPermission);
  const { policy } = useAutomationPolicy();
  const [saving, setSaving] = useState(false);
  const blocked = permission === "denied" || permission === "unsupported";
  const setFlag = async (patch: Partial<LocalAiPolicy>) => {
    if (!policy) return;
    setSaving(true);
    try {
      if (Object.values(patch).some(Boolean)) setPermission(await ensureNotificationPermission());
      await saveAutomationPolicy({ ...policy, ...patch });
    } catch (error) {
      toast({ title: t("settingsD3.updates.saveFailed"), description: errorText(error), variant: "destructive" });
    } finally {
      setSaving(false);
    }
  };
  const flagRow = (key: "notify_on_improvement" | "notify_on_new_insight" | "notify_daily_summary", titleKey: string, hintKey: string) => (
    <SummaryRow
      title={t(titleKey)}
      summary={t(hintKey)}
      action={
        <Switch
          aria-label={t(titleKey)}
          disabled={!policy || saving || blocked}
          title={blocked ? t(`settingsD3.notifications.permission.${permission}`) : undefined}
          checked={Boolean(policy?.[key]) && !blocked}
          onCheckedChange={(next) => void setFlag({ [key]: next })}
        />
      }
    />
  );

  // A permissão muda fora do app (o Windows pergunta na primeira vez).
  useEffect(() => {
    const id = window.setInterval(() => setPermission(readNotificationPermission()), 10_000);
    return () => window.clearInterval(id);
  }, []);

  return (
    <Card label={t("settingsD3.notifications.label")}>
      <SummaryRow
        title={t("settingsD3.notifications.critical")}
        summary={t("settingsD3.notifications.criticalHint")}
        state={
          <StatusPill tone={permission === "granted" ? "ok" : permission === "denied" ? "warn" : "muted"}>
            {t(`settingsD3.notifications.permission.${permission}`)}
          </StatusPill>
        }
      />
      {flagRow("notify_on_improvement", "settingsD3.notifications.improve", "settingsD3.notifications.improveHint")}
      {flagRow("notify_on_new_insight", "settingsD3.notifications.newInsight", "settingsD3.notifications.newInsightHint")}
      {flagRow("notify_daily_summary", "settingsD3.notifications.daily", "settingsD3.notifications.dailyHint")}
      <SummaryRow title={t("settingsD3.notifications.bell")} summary={t("settingsD3.notifications.bellHint")} />
      <div className="mt-3 flex flex-wrap items-center justify-between gap-x-4 gap-y-1">
        <p className="text-xs leading-relaxed text-fg-3">{t("settingsD3.notifications.rule")}</p>
        {onOpenHistory && (
          <Button size="sm" variant="link" onClick={onOpenHistory}>
            {t("settingsD3.notifications.openHistory")}
          </Button>
        )}
      </div>
    </Card>
  );
}

/* ------------------------------------------------------------------ */
/* Privacidade                                                         */
/* ------------------------------------------------------------------ */

const PRIVACY_DETAILS = ["sample", "actions", "programs", "local"] as const;

function PrivacyCard({
  track,
  onOpenAccountSettings,
}: {
  track: Track;
  onOpenAccountSettings: () => Promise<void>;
}) {
  const { t } = useI18n();
  const [showAll, setShowAll] = useState(false);
  const [uiTelemetry, setUiTelemetry] = useState(isTelemetryEnabled);
  const queue = useUiTelemetryQueue();
  const queueSize = queue.size;
  const didMount = useRef(false);
  // Opt-out da leitura detalhada (doc 12 §2): a mesma política do agente
  // que Controles › Automações grava; desligado, o agente manda só o resumo.
  const { policy } = useAutomationPolicy();
  const [savingDetailed, setSavingDetailed] = useState(false);
  const setDetailed = async (enabled: boolean) => {
    if (!policy) return;
    setSavingDetailed(true);
    try {
      await saveAutomationPolicy({ ...policy, telemetry_diagnostics_enabled: enabled });
      track("detailed_telemetry_preference_changed", { enabled });
    } catch (error) {
      toast({ title: t("settingsD3.updates.saveFailed"), description: errorText(error), variant: "destructive" });
    } finally {
      setSavingDetailed(false);
    }
  };

  useEffect(() => {
    setTelemetryEnabled(uiTelemetry);
    if (didMount.current) {
      track("ui_telemetry_preference_changed", { enabled: uiTelemetry });
      queue.refresh();
    }
    didMount.current = true;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [uiTelemetry, track]);

  return (
    <Card label={t("settingsD3.privacy.label")}>
      <div className="border-b border-line-soft py-3.5">
        <div className="flex items-start gap-4">
          <div className="min-w-0 flex-1">
            <div className="text-[15px] font-semibold leading-snug text-fg">{t("settingsD3.privacy.leaves")}</div>
            <div className="mt-0.5 text-[13px] leading-snug text-fg-3">{t("settingsD3.privacy.leavesHint")}</div>
          </div>
          <Button size="sm" variant="link" aria-expanded={showAll} onClick={() => setShowAll((value) => !value)}>
            {showAll ? t("settingsD3.privacy.hide") : t("settingsD3.privacy.seeAll")}
          </Button>
        </div>
        {showAll && (
          <ul className="mt-3 flex list-disc flex-col gap-1.5 pl-5 text-[13px] leading-snug text-fg-2">
            {PRIVACY_DETAILS.map((key) => (
              <li key={key}>{t(`settingsD3.privacy.details.${key}`)}</li>
            ))}
          </ul>
        )}
      </div>
      <SummaryRow
        title={t("settingsD3.privacy.detailed")}
        summary={t("settingsD3.privacy.detailedHint")}
        action={
          <Switch
            aria-label={t("settingsD3.privacy.detailed")}
            checked={policy?.telemetry_diagnostics_enabled ?? true}
            disabled={!policy || savingDetailed}
            onCheckedChange={(next) => void setDetailed(next)}
          />
        }
      />
      <SummaryRow
        title={t("settingsD3.privacy.uiUsage")}
        summary={
          <>
            {t("settingsD3.privacy.uiUsageHint")}
            <span className="num mt-0.5 block text-xs text-fg-4">
              {queueSize > 0
                ? t("settingsD3.privacy.queue", { count: queueSize })
                : t("settingsD3.privacy.queueEmpty")}
            </span>
          </>
        }
        action={
          <Switch aria-label={t("settingsD3.privacy.uiUsage")} checked={uiTelemetry} onCheckedChange={setUiTelemetry} />
        }
      />
      <SummaryRow
        title={t("settingsD3.privacy.exportDelete")}
        summary={t("settingsD3.privacy.exportDeleteHint")}
        action={
          <Button size="sm" variant="outline" onClick={() => void onOpenAccountSettings().catch(() => undefined)}>
            <ExternalLink aria-hidden />
            {t("settingsD3.privacy.openSite")}
          </Button>
        }
      />
    </Card>
  );
}

/* ------------------------------------------------------------------ */
/* Conta e PCs                                                         */
/* ------------------------------------------------------------------ */

function planLabel(plan: string) {
  const normalized = plan.trim().toLowerCase();
  if (!normalized || normalized === "free") return "Starter";
  return normalized.slice(0, 1).toUpperCase() + normalized.slice(1);
}


function AccountCard({
  user,
  status,
  syncingPlan,
  onLogin,
  onLogout,
  onOpenAccountSettings,
  onOpenBilling,
  onSyncPlan,
}: Omit<SettingsProps, "onOpenHistory" | "onOpenControls">) {
  const { t } = useI18n();
  const weekly = useWeeklyAutomationUsage();

  if (!user) {
    return (
      <Card label={t("settingsD3.account.label")}>
        <EmptyState
          title={t("settingsD3.account.loggedOutTitle")}
          reason={t("settingsD3.account.loggedOutReason")}
          action={
            <Button variant="primary" onClick={() => void onLogin().catch(() => undefined)}>
              {t("settingsD3.account.login")}
              <ExternalLink aria-hidden />
            </Button>
          }
        />
        <Foot>{t("settingsD3.account.foot")}</Foot>
      </Card>
    );
  }

  const isReady = Boolean(status?.authenticated && status.registered);
  const syncLine = status?.plan_sync_error
    ? t("settingsD3.account.planSyncFailed", { category: status.plan_sync_error })
    : status?.plan_synced_at
      ? t("settingsD3.account.planSyncOk", { time: formatAgo(status.plan_synced_at) })
      : t("settingsD3.account.planSyncNever");
  const hasWeeklyLimit = weekly != null && weekly.limitSeconds != null;

  return (
    <Card label={t("settingsD3.account.label")}>
      <SummaryRow
        title={user.email || user.name}
        summary={user.email ? user.name : t("settingsD3.account.noEmail")}
        state={
          <StatusPill tone={isReady ? "ok" : "muted"}>
            {isReady ? t("settingsD3.account.online") : t("settingsD3.account.pending")}
          </StatusPill>
        }
      />
      <SummaryRow
        title={t("settingsD3.account.plan", { plan: planLabel(user.plan) })}
        summary={
          <>
            <span className={status?.plan_sync_error ? "text-warn" : undefined}>{syncLine}</span>
            {hasWeeklyLimit && (
              <span className="num mt-0.5 block">
                {weekly.limitReached
                  ? t("settingsD3.account.weeklyUsedUp")
                  : t("settingsD3.account.weeklyUsage", {
                      used: formatMinutes(weekly.usedSeconds),
                      limit: formatMinutes(weekly.limitSeconds),
                    })}
              </span>
            )}
          </>
        }
        action={
          <Button size="sm" variant="outline" disabled={syncingPlan} onClick={() => void onSyncPlan().catch(() => undefined)}>
            <RefreshCw className={syncingPlan ? "animate-spin" : undefined} aria-hidden />
            {syncingPlan ? t("settingsD3.account.syncingPlan") : t("settingsD3.account.syncPlan")}
          </Button>
        }
      />
      <DeviceList currentHwId={status?.hw_id ?? null} enabled={isReady} onLogout={onLogout} />
      <div className="flex flex-wrap items-center gap-2 pt-3.5">
        <Button size="sm" variant="outline" onClick={() => void onOpenAccountSettings().catch(() => undefined)}>
          <ExternalLink aria-hidden />
          {t("settingsD3.account.manage")}
        </Button>
        {!user.hasPaidPlan && (
          <Button size="sm" variant="outline" onClick={() => void onOpenBilling().catch(() => undefined)}>
            <ExternalLink aria-hidden />
            {t("settingsD3.account.becomePro")}
          </Button>
        )}
        <Button size="sm" variant="outline" className="ml-auto" onClick={() => void onLogout().catch(() => undefined)}>
          <LogOut aria-hidden />
          {t("settingsD3.account.logout")}
        </Button>
      </div>
      <Foot>{t("settingsD3.account.foot")}</Foot>
    </Card>
  );
}

/* ------------------------------------------------------------------ */
/* Atualizações (+ abrir com o Windows)                                */
/* ------------------------------------------------------------------ */

function updateSummary(status: UpdateStatus | null, t: ReturnType<typeof useI18n>["t"]) {
  if (!status) return t("settingsD3.updates.neverChecked");
  if (status.available) return t("settingsD3.updates.available", { version: status.version ?? "" });
  if (status.lastCheckedAt) return t("settingsD3.updates.checkedAgo", { time: formatAgo(status.lastCheckedAt) });
  return t("settingsD3.updates.upToDate");
}

function UpdatesCard({ track }: { track: Track }) {
  const { t } = useI18n();
  const updater = useUpdater();
  const { policy } = useAutomationPolicy();
  const [checking, setChecking] = useState(false);
  const [popup, setPopup] = useState<"available" | "up_to_date" | null>(null);
  const status = updater.status;
  const version = status?.currentVersion?.trim();

  const checkNow = () => {
    setChecking(true);
    updater
      .check()
      .then((next) => {
        track("update_check_requested", { available: next.available });
        setPopup(next.available ? "available" : "up_to_date");
      })
      .catch(() => undefined)
      .finally(() => setChecking(false));
  };

  const savePolicy = (patch: Partial<LocalAiPolicy>) => {
    if (!policy) return;
    const previous = policy;
    const next = { ...policy, ...patch };
    automationPolicy.set(next);
    saveAutomationPolicy(next)
      .then((saved) => track("local_ai_policy_saved", { enabled: saved.enabled, maxRisk: saved.max_risk }))
      .catch((error) => {
        automationPolicy.set(previous);
        toast({ title: t("settingsD3.updates.saveFailed"), description: errorText(error), variant: "destructive" });
      });
  };

  const loadingReason = policy ? null : t("settingsD3.updates.policyLoading");

  return (
    <Card
      label={t("settingsD3.updates.label")}
      aside={status?.available ? <StatusPill tone="ok">{t("settingsD3.updates.availablePill")}</StatusPill> : null}
    >
      <SummaryRow
        title={
          version ? (
            <span className="num">{t("settingsD3.updates.version", { version })}</span>
          ) : (
            <EmptyState
              variant="inline"
              title={t("settingsD3.updates.versionUnknownTitle")}
              reason={t("settingsD3.updates.versionUnknownReason")}
            />
          )
        }
        summary={
          <>
            {updateSummary(status, t)}
            {status?.lastError && (
              <span className="mt-0.5 block text-warn">{t("settingsD3.updates.lastError", { error: status.lastError })}</span>
            )}
          </>
        }
        action={
          <Button size="sm" variant="outline" disabled={checking} onClick={checkNow}>
            <RefreshCw className={checking ? "animate-spin" : undefined} aria-hidden />
            {checking ? t("settingsD3.updates.checking") : t("settingsD3.updates.check")}
          </Button>
        }
      />
      <SummaryRow
        title={t("settingsD3.updates.autostart")}
        summary={t("settingsD3.updates.autostartHint")}
        action={
          <Switch
            aria-label={t("settingsD3.updates.autostart")}
            title={loadingReason ?? undefined}
            disabled={!policy}
            checked={policy?.autostart_enabled ?? false}
            onCheckedChange={(autostart_enabled) => savePolicy({ autostart_enabled })}
          />
        }
      />
      <SummaryRow
        title={t("settingsD3.updates.startMinimized")}
        summary={t("settingsD3.updates.startMinimizedHint")}
        action={
          <Switch
            aria-label={t("settingsD3.updates.startMinimized")}
            title={loadingReason ?? undefined}
            disabled={!policy}
            checked={policy?.start_minimized ?? false}
            onCheckedChange={(start_minimized) => savePolicy({ start_minimized })}
          />
        }
      />
      <SummaryRow
        title={t("settingsD3.updates.autoInstall")}
        summary={t("settingsD3.updates.autoInstallHint")}
        action={
          <Switch
            aria-label={t("settingsD3.updates.autoInstall")}
            title={loadingReason ?? undefined}
            disabled={!policy}
            checked={policy?.auto_install_updates ?? false}
            onCheckedChange={(auto_install_updates) => savePolicy({ auto_install_updates })}
          />
        }
      />

      {popup && (
        <UpdateCheckPopup
          result={popup}
          status={status}
          busy={Boolean(status?.installing)}
          onUpdateNow={() => {
            void updater.apply().catch(() => undefined);
            setPopup(null);
          }}
          onClose={() => setPopup(null)}
        />
      )}
    </Card>
  );
}

/** O mesmo resultado de "Verificar" da tela antiga, com as peças D3. */
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
        className="w-full max-w-md rounded-card border border-line bg-surface p-6"
      >
        <div className="label-mono">{t("settingsD3.popup.eyebrow")}</div>
        {result === "available" ? (
          <>
            <h2 className="mt-2 text-xl font-semibold text-fg">
              {mandatory
                ? t("update.availableTitleMandatory", { version: status?.version ?? "" })
                : t("update.availableTitle", { version: status?.version ?? "" })}
            </h2>
            <p className="mt-2 text-sm leading-relaxed text-fg-3">{status?.notes?.trim() || t("update.notesFallback")}</p>
            {mandatory && <p className="mt-2 text-xs font-medium text-danger">{t("update.mandatoryNotice")}</p>}
            <div className="mt-6 flex justify-end gap-2">
              {!mandatory && (
                <Button size="sm" variant="outline" onClick={onClose}>
                  {t("update.later")}
                </Button>
              )}
              <Button size="sm" variant="outline" disabled={busy} onClick={onUpdateNow}>
                {busy ? t("update.installing") : t("update.updateNow")}
              </Button>
            </div>
          </>
        ) : (
          <>
            <h2 className="mt-2 text-xl font-semibold text-fg">{t("update.upToDate")}</h2>
            <p className="num mt-2 text-sm leading-relaxed text-fg-3">
              {t("settingsD3.popup.upToDateBody", {
                version: status?.currentVersion?.trim() || t("settingsD3.popup.versionUnknown"),
              })}
            </p>
            <div className="mt-6 flex justify-end">
              <Button size="sm" variant="outline" onClick={onClose}>
                {t("common.close")}
              </Button>
            </div>
          </>
        )}
      </section>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* PCs da conta                                                        */
/* ------------------------------------------------------------------ */

function DeviceList({ currentHwId, enabled, onLogout }: { currentHwId: string | null; enabled: boolean; onLogout: () => Promise<void> }) {
  const { t } = useI18n();
  const { devices, loading, error } = useAccountDevices({ auto: enabled });
  const [confirming, setConfirming] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const remove = async (id: string) => {
    setConfirming(null);
    setBusy(id);
    try {
      await removeDevice(id);
      toast({ title: t("settingsD3.devices.removed") });
      if (id === currentHwId) await onLogout().catch(() => undefined);
    } catch (err) {
      toast({ title: t("settingsD3.devices.removeFailed"), description: errorText(err), variant: "destructive" });
    } finally {
      setBusy(null);
    }
  };

  const lastSeen = (device: { last_ping?: string | null; registered_at?: string | null }) => {
    const iso = device.last_ping ?? device.registered_at;
    const seconds = iso ? Math.floor(Date.parse(iso) / 1000) : null;
    return seconds && Number.isFinite(seconds) ? t("settingsD3.devices.lastSeen", { time: formatAgo(seconds) }) : null;
  };

  if (!enabled) return null;
  return (
    <div className="border-t border-line-soft pt-3.5">
      <div className="label-mono">{t("settingsD3.devices.label")}</div>
      {devices.length === 0 ? (
        <p className="mt-2 text-[13px] text-fg-3">{loading ? t("settingsD3.devices.loading") : error ?? t("settingsD3.devices.empty")}</p>
      ) : (
        <ul className="mt-2 flex flex-col">
          {devices.map((device) => {
            const isThis = device.id === currentHwId;
            const name = device.name?.trim() || device.machine_type?.trim() || t("settingsD3.devices.unnamed");
            return (
              <li key={device.id} className="flex flex-col gap-2 border-b border-line-soft py-2.5 last:border-b-0">
                <div className="flex items-center gap-3">
                  <Monitor className="h-4 w-4 shrink-0 text-fg-3" aria-hidden />
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2 text-[15px] font-medium text-fg">
                      <span className="truncate">{name}</span>
                      {isThis && <StatusPill tone="ok">{t("settingsD3.devices.thisPc")}</StatusPill>}
                      {!isThis && device.is_active && <StatusPill>{t("settingsD3.devices.active")}</StatusPill>}
                    </div>
                    <div className="truncate text-xs text-fg-3">
                      {[device.os_version, lastSeen(device)].filter(Boolean).join(" · ")}
                    </div>
                  </div>
                  <Button size="sm" variant="outline" disabled={busy !== null} onClick={() => setConfirming(device.id)}>
                    <Trash2 aria-hidden />
                    {t("settingsD3.devices.remove")}
                  </Button>
                </div>
                {confirming === device.id && (
                  <ConfirmInline
                    danger
                    busy={busy === device.id}
                    message={isThis ? t("settingsD3.devices.confirmThis") : t("settingsD3.devices.confirmOther", { name })}
                    confirmLabel={t("settingsD3.devices.remove")}
                    onConfirm={() => void remove(device.id)}
                    onCancel={() => setConfirming(null)}
                  />
                )}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Linha que leva a Controles                                          */
/* ------------------------------------------------------------------ */

function MovedCard({ onOpenControls, onOpenSystemComponent }: { onOpenControls?: () => void; onOpenSystemComponent?: () => void }) {
  const { t } = useI18n();
  const helper = useHelper();
  const state = !helper.status ? null : helper.ready ? "ready" : helper.status.installed ? "attention" : "missing";
  return (
    <Card>
      <div className="flex items-center justify-between gap-4">
        <div className="min-w-0">
          <div className="text-[15px] font-semibold leading-snug text-fg">{t("settingsD3.moved.title")}</div>
          <div className="mt-0.5 text-[13px] leading-snug text-fg-3">{t("settingsD3.moved.body")}</div>
        </div>
        {onOpenControls && (
          <Button size="sm" variant="link" onClick={onOpenControls}>
            {t("settingsD3.moved.action")}
          </Button>
        )}
      </div>
      <div className="mt-3 flex items-center justify-between gap-4 border-t border-line-soft pt-3">
        <div className="min-w-0">
          <div className="text-[15px] font-semibold leading-snug text-fg">{t("settingsD3.moved.componentTitle")}</div>
          <div className="mt-0.5 text-[13px] leading-snug text-fg-3">
            {state ? t(`settingsD3.moved.componentState.${state}`, { version: helper.status?.version ?? "" }) : t("settingsD3.moved.componentReading")}
          </div>
        </div>
        {onOpenSystemComponent && (
          <Button size="sm" variant="link" onClick={onOpenSystemComponent}>
            {t("settingsD3.moved.componentAction")}
          </Button>
        )}
      </div>
    </Card>
  );
}
