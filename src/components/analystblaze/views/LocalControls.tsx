import { BatteryCharging, History, ListChecks, RefreshCw, Shield, ShieldCheck, Wifi, Wrench } from "lucide-react";
import type { ReactNode } from "react";
import { useEffect, useMemo, useState } from "react";
import { useI18n } from "@/i18n";
import { useTelemetry } from "@/hooks/useTelemetry";
import {
  addProtectedApp,
  getAuditLog,
  getEnergyDiagnostics,
  getNetworkDiagnostics,
  getOptimizationSnapshots,
  getPrivilegedHelperStatus,
  getProtectedApps,
  getWindowsInventory,
  isTauriRuntime,
  removeProtectedApp,
  type AuditEvent,
  type EnergyDiagnostics,
  type NetworkDiagnostics,
  type OptimizationSnapshot,
  type PrivilegedHelperStatus,
  type ProtectedApp,
  type WindowsInventory,
} from "@/services/tauri/agent";

export function LocalControls({
  busy,
  onRestoreOptimizations,
  onDisableStartup,
  onRestoreStartup,
  onStopService,
  onRestoreService,
  onSetPowerPlan,
}: {
  busy: boolean;
  onRestoreOptimizations: () => Promise<unknown>;
  onDisableStartup: (name: string, location?: string | null) => Promise<unknown>;
  onRestoreStartup: (name?: string | null) => Promise<unknown>;
  onStopService: (name: string) => Promise<unknown>;
  onRestoreService: (name?: string | null) => Promise<unknown>;
  onSetPowerPlan: (plan: "high_performance" | "balanced" | "power_saver") => Promise<unknown>;
}) {
  const { t } = useI18n();
  const track = useTelemetry("local_controls");
  const [inventory, setInventory] = useState<WindowsInventory>({ startup_apps: [], services: [] });
  const [inventoryBusy, setInventoryBusy] = useState(false);
  const [auditEvents, setAuditEvents] = useState<AuditEvent[]>([]);
  const [snapshots, setSnapshots] = useState<OptimizationSnapshot[]>([]);
  const [protectedApps, setProtectedApps] = useState<ProtectedApp[]>([]);
  const [protectedInput, setProtectedInput] = useState("");
  const [helperStatus, setHelperStatus] = useState<PrivilegedHelperStatus | null>(null);
  const [networkDiagnostics, setNetworkDiagnostics] = useState<NetworkDiagnostics | null>(null);
  const [energyDiagnostics, setEnergyDiagnostics] = useState<EnergyDiagnostics | null>(null);
  const [diagnosticsBusy, setDiagnosticsBusy] = useState(false);
  const [diagnosticsError, setDiagnosticsError] = useState<string | null>(null);
  const [inventoryError, setInventoryError] = useState<string | null>(null);
  const [historyError, setHistoryError] = useState<string | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const runtimeAvailable = isTauriRuntime();

  const visibleStartupApps = useMemo(
    () => inventory.startup_apps.filter((app) => app.risk === "safe").slice(0, 8),
    [inventory.startup_apps],
  );
  const visibleServices = useMemo(
    () => inventory.services.filter((service) => service.can_modify && service.classification === "safe").slice(0, 8),
    [inventory.services],
  );

  useEffect(() => {
    void refreshWindowsInventory();
    void refreshOperationalHistory();
    void refreshNetworkAndEnergy();
  }, []);

  const refreshWindowsInventory = async () => {
    setInventoryBusy(true);
    setInventoryError(null);
    try {
      setInventory(await getWindowsInventory());
      track("windows_inventory_refreshed");
    } catch (error) {
      setInventory({ startup_apps: [], services: [] });
      setInventoryError(errorMessage(error));
    } finally {
      setInventoryBusy(false);
    }
  };

  const refreshOperationalHistory = async () => {
    setHistoryError(null);
    try {
      const [nextAudit, nextSnapshots, nextProtected, nextHelper] = await Promise.all([
        getAuditLog(120),
        getOptimizationSnapshots(120),
        getProtectedApps(),
        getPrivilegedHelperStatus(),
      ]);
      setAuditEvents(nextAudit);
      setSnapshots(nextSnapshots);
      setProtectedApps(nextProtected);
      setHelperStatus(nextHelper);
      track("local_history_refreshed");
    } catch (error) {
      setAuditEvents([]);
      setSnapshots([]);
      setProtectedApps([]);
      setHelperStatus(null);
      setHistoryError(errorMessage(error));
    }
  };

  const refreshNetworkAndEnergy = async () => {
    setDiagnosticsBusy(true);
    setDiagnosticsError(null);
    try {
      const [nextNetwork, nextEnergy] = await Promise.all([
        getNetworkDiagnostics(),
        getEnergyDiagnostics(),
      ]);
      setNetworkDiagnostics(nextNetwork);
      setEnergyDiagnostics(nextEnergy);
      track("network_energy_refreshed");
    } catch (error) {
      setNetworkDiagnostics(null);
      setEnergyDiagnostics(null);
      setDiagnosticsError(errorMessage(error));
    } finally {
      setDiagnosticsBusy(false);
    }
  };

  const addProtected = async () => {
    const name = protectedInput.trim();
    if (!name) return;
    setActionMessage(null);
    try {
      setProtectedApps(await addProtectedApp(name, "user"));
      setProtectedInput("");
      setActionMessage(t("controls.protectedAdded", { name }));
      track("protected_app_added", { name });
    } catch (error) {
      setActionMessage(errorMessage(error));
    }
  };

  const removeProtected = async (name: string) => {
    setActionMessage(null);
    try {
      setProtectedApps(await removeProtectedApp(name));
      setActionMessage(t("controls.protectedRemoved", { name }));
      track("protected_app_removed", { name });
    } catch (error) {
      setActionMessage(errorMessage(error));
    }
  };

  const runControlAction = async (action: () => Promise<unknown>, successMessage: string) => {
    setActionMessage(null);
    try {
      const result = await action();
      if (result === false) {
        setActionMessage(t("controls.actionCancelled"));
        return;
      }
      setActionMessage(successMessage);
    } catch (error) {
      setActionMessage(errorMessage(error));
    }
  };

  const applyPowerPlan = async (plan: "high_performance" | "balanced" | "power_saver") => {
    await runControlAction(
      async () => {
        const result = await onSetPowerPlan(plan);
        if (result === false) return false;
        await refreshNetworkAndEnergy();
        await refreshOperationalHistory();
      },
      "Plano de energia atualizado.",
    );
  };

  return (
    <div className="flex flex-col gap-8">
      <header className="flex flex-col gap-2">
        <div className="font-mono text-[11px] uppercase tracking-[0.3em] text-cyan-400/70">
          {t("controls.eyebrow")}
        </div>
        <h1 className="text-[36px] font-semibold tracking-tight text-slate-50">
          {t("controls.title")}
        </h1>
        <p className="max-w-3xl text-sm text-slate-400">{t("controls.description")}</p>
      </header>

      {!runtimeAvailable && (
        <Notice tone="warning" message={t("controls.tauriRequired")} />
      )}
      {actionMessage && <Notice tone="info" message={actionMessage} />}

      <section className="glass-panel cyber-glow p-6">
        <div className="flex flex-col gap-3 pb-4 md:flex-row md:items-center md:justify-between">
          <div className="flex items-center gap-2">
            <Wifi className="h-3.5 w-3.5 text-cyan-300" />
            <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-cyan-400/80">Rede e energia</h2>
          </div>
          <button
            disabled={diagnosticsBusy}
            onClick={() => void refreshNetworkAndEnergy()}
            className="inline-flex items-center gap-2 rounded-xl border border-cyan-400/40 bg-cyan-400/10 px-3 py-2 text-xs font-medium text-cyan-100 transition-all hover:border-cyan-300/60 disabled:opacity-50"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${diagnosticsBusy ? "animate-spin" : ""}`} />
            {t("common.refresh")}
          </button>
        </div>
        {diagnosticsError && <Notice tone="danger" message={diagnosticsError} />}
        <div className="grid gap-4 lg:grid-cols-2">
          <DiagnosticsPanel
            icon={<Wifi className="h-4 w-4 text-cyan-300" />}
            title="Diagnostico de rede"
            rows={[
              ["Status", networkDiagnostics ? (networkDiagnostics.connected ? "online" : "offline") : "--"],
              ["Adaptador", networkDiagnostics?.adapter_name ?? "--"],
              ["Tipo", networkDiagnostics?.adapter_type ?? networkDiagnostics?.adapter_description ?? "--"],
              ["Link", networkDiagnostics?.link_speed ?? "--"],
              ["Wi-Fi", networkDiagnostics?.wifi_ssid ? `${networkDiagnostics.wifi_ssid}${networkDiagnostics.wifi_signal_percent != null ? ` - ${Math.round(networkDiagnostics.wifi_signal_percent)}%` : ""}` : "--"],
              ["Ping externo", formatMs(networkDiagnostics?.external_latency_ms)],
              ["Jitter", formatMs(networkDiagnostics?.jitter_ms)],
              ["Perda", formatPercent(networkDiagnostics?.packet_loss_percent)],
            ]}
            footer={networkDiagnostics?.recommendations?.join(" / ") ?? "Aguardando leitura real do Windows."}
          />
          <DiagnosticsPanel
            icon={<BatteryCharging className="h-4 w-4 text-cyan-300" />}
            title="Energia"
            rows={[
              ["Plano ativo", energyPlanLabel(energyDiagnostics?.active_scheme_alias, energyDiagnostics?.active_scheme_name)],
              ["Fonte", powerSourceLabel(energyDiagnostics?.power_source)],
              ["Bateria", energyDiagnostics?.battery_percent != null ? `${Math.round(energyDiagnostics.battery_percent)}% ${energyDiagnostics.battery_status ?? ""}` : "--"],
              ["Economia", energyDiagnostics?.battery_saver_on == null ? "--" : energyDiagnostics.battery_saver_on ? "ativa" : "inativa"],
              ["Clock CPU", formatClock(energyDiagnostics?.cpu_current_clock_mhz, energyDiagnostics?.cpu_max_clock_mhz)],
              ["Recomendado", energyPlanLabel(energyDiagnostics?.recommended_plan, null)],
            ]}
            footer={energyDiagnostics?.recommendations?.join(" / ") ?? "Aguardando leitura real do Windows."}
            actions={
              <div className="flex flex-wrap gap-2">
                <PowerButton disabled={busy || diagnosticsBusy} onClick={() => void applyPowerPlan("balanced")} label="Equilibrado" />
                <PowerButton disabled={busy || diagnosticsBusy} onClick={() => void applyPowerPlan("high_performance")} label="Desempenho" />
                <PowerButton disabled={busy || diagnosticsBusy} onClick={() => void applyPowerPlan("power_saver")} label="Economia" />
              </div>
            }
          />
        </div>
      </section>

      <section className="glass-panel cyber-glow p-6">
        <div className="flex flex-col gap-3 pb-4 md:flex-row md:items-center md:justify-between">
          <div className="flex items-center gap-2">
            <ListChecks className="h-3.5 w-3.5 text-cyan-300" />
            <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-cyan-400/80">{t("settings.windowsControls")}</h2>
          </div>
          <button
            disabled={inventoryBusy}
            onClick={() => void refreshWindowsInventory()}
            className="inline-flex items-center gap-2 rounded-xl border border-cyan-400/40 bg-cyan-400/10 px-3 py-2 text-xs font-medium text-cyan-100 transition-all hover:border-cyan-300/60 disabled:opacity-50"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${inventoryBusy ? "animate-spin" : ""}`} />
            {t("common.refresh")}
          </button>
        </div>
        {inventoryError && <Notice tone="danger" message={inventoryError} />}
        <div className="grid gap-4 lg:grid-cols-2">
          <InventoryPanel
            title={t("settings.startupApps")}
            empty={t("settings.noSafeStartupApps")}
            items={visibleStartupApps.map((app) => ({
              key: `${app.location}:${app.name}`,
              title: app.name,
              subtitle: app.location,
              badge: app.risk,
              action: t("settings.disableStartup"),
              restore: t("settings.restoreStartup"),
              onAction: async () => {
                await runControlAction(
                  async () => {
                    const result = await onDisableStartup(app.name, app.location);
                    if (result === false) return false;
                    await refreshWindowsInventory();
                    await refreshOperationalHistory();
                  },
                  t("controls.actionCompleted"),
                );
              },
              onRestore: async () => {
                await runControlAction(
                  async () => {
                    const result = await onRestoreStartup(app.name);
                    if (result === false) return false;
                    await refreshWindowsInventory();
                    await refreshOperationalHistory();
                  },
                  t("controls.actionCompleted"),
                );
              },
            }))}
            disabled={busy || inventoryBusy}
          />
          <InventoryPanel
            title={t("settings.windowsServices")}
            empty={t("settings.noSafeServices")}
            items={visibleServices.map((service) => ({
              key: service.name,
              title: service.display_name || service.name,
              subtitle: service.name,
              badge: service.classification,
              action: t("settings.stopService"),
              restore: t("settings.restoreService"),
              onAction: async () => {
                await runControlAction(
                  async () => {
                    const result = await onStopService(service.name);
                    if (result === false) return false;
                    await refreshWindowsInventory();
                    await refreshOperationalHistory();
                  },
                  t("controls.actionCompleted"),
                );
              },
              onRestore: async () => {
                await runControlAction(
                  async () => {
                    const result = await onRestoreService(service.name);
                    if (result === false) return false;
                    await refreshWindowsInventory();
                    await refreshOperationalHistory();
                  },
                  t("controls.actionCompleted"),
                );
              },
            }))}
            disabled={busy || inventoryBusy}
          />
        </div>
      </section>

      <section className="glass-panel cyber-glow p-6">
        <div className="flex flex-col gap-3 pb-4 md:flex-row md:items-center md:justify-between">
          <div className="flex items-center gap-2">
            <Shield className="h-3.5 w-3.5 text-cyan-300" />
            <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-cyan-400/80">{t("settings.protectedApps")}</h2>
          </div>
          <button
            onClick={() => void refreshOperationalHistory()}
            className="inline-flex items-center gap-2 rounded-xl border border-cyan-400/40 bg-cyan-400/10 px-3 py-2 text-xs font-medium text-cyan-100 transition-all hover:border-cyan-300/60"
          >
            <RefreshCw className="h-3.5 w-3.5" />
            {t("common.refresh")}
          </button>
        </div>
        {historyError && <Notice tone="danger" message={historyError} />}
        <div className="flex flex-col gap-3 md:flex-row">
          <input
            value={protectedInput}
            onChange={(event) => setProtectedInput(event.target.value)}
            placeholder={t("settings.protectedAppPlaceholder")}
            className="min-h-11 flex-1 rounded-xl border border-cyan-500/20 bg-slate-950/60 px-3 text-sm text-slate-100 outline-none transition focus:border-cyan-300/60"
          />
          <button
            onClick={() => void addProtected()}
            className="rounded-xl border border-cyan-400/40 bg-cyan-400/10 px-4 py-2.5 text-sm font-semibold text-cyan-100 transition hover:bg-cyan-400/15"
          >
            {t("settings.addProtectedApp")}
          </button>
        </div>
        <div className="mt-4 flex flex-wrap gap-2">
          {protectedApps.slice(0, 40).map((app) => (
            <button
              key={`${app.name}:${app.created_at}`}
              onClick={() => app.created_at > 0 && void removeProtected(app.name)}
              className="rounded-md border border-cyan-500/15 bg-slate-950/60 px-2.5 py-1 font-mono text-[10px] uppercase tracking-widest text-slate-300 transition hover:border-rose-400/40 hover:text-rose-200"
              title={app.created_at > 0 ? t("settings.removeProtectedApp") : t("settings.defaultProtectedApp")}
            >
              {app.name}
            </button>
          ))}
        </div>
      </section>

      <section className="glass-panel cyber-glow p-6">
        <div className="flex flex-col gap-3 pb-4 md:flex-row md:items-center md:justify-between">
          <div className="flex items-center gap-2">
            <History className="h-3.5 w-3.5 text-cyan-300" />
            <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-cyan-400/80">{t("settings.history")}</h2>
          </div>
          <div className="flex flex-wrap gap-2">
            <button
              onClick={() => void refreshOperationalHistory()}
              className="inline-flex items-center gap-2 rounded-xl border border-cyan-400/40 bg-cyan-400/10 px-3 py-2 text-xs font-medium text-cyan-100 transition-all hover:border-cyan-300/60"
            >
              <RefreshCw className="h-3.5 w-3.5" />
              {t("common.refresh")}
            </button>
            <button
              disabled={busy}
              onClick={() => void runControlAction(
                async () => {
                  const result = await onRestoreOptimizations();
                  if (result === false) return false;
                  await refreshOperationalHistory();
                  await refreshWindowsInventory();
                },
                t("controls.restoreCompleted"),
              )}
              className="inline-flex items-center gap-2 rounded-xl border border-emerald-400/40 bg-emerald-400/10 px-3 py-2 text-xs font-medium text-emerald-100 transition-all hover:border-emerald-300/60 disabled:opacity-50"
            >
              <ShieldCheck className="h-3.5 w-3.5" />
              {t("settings.restoreSnapshots")}
            </button>
          </div>
        </div>
        <div className="grid gap-4 lg:grid-cols-2">
          <HistoryPanel
            title={t("settings.snapshots")}
            empty={t("settings.noSnapshots")}
            rows={snapshots.slice(0, 10).map((snapshot) => ({
              key: snapshot.id,
              title: snapshot.action_name,
              detail: `${snapshot.entries.length} ${t("settings.entries")} - ${snapshot.restored_at ? t("settings.restored") : t("settings.pendingRestore")}`,
              time: snapshot.created_at,
            }))}
          />
          <HistoryPanel
            title={t("settings.auditLog")}
            empty={t("settings.noAuditEvents")}
            rows={auditEvents.slice(0, 10).map((event) => ({
              key: `${event.timestamp}:${event.event}`,
              title: event.event,
              detail: event.message,
              time: event.timestamp,
            }))}
          />
        </div>
        <div className="mt-4 rounded-xl border border-cyan-500/10 bg-slate-950/40 p-4">
          <div className="flex items-center gap-2 text-sm font-semibold text-slate-100">
            <Wrench className="h-4 w-4 text-cyan-300" />
            {t("settings.privilegedHelper")}
          </div>
          <p className="mt-1 text-xs text-slate-500">
            {helperStatus?.message ?? t("settings.helperUnknown")}
          </p>
        </div>
      </section>
    </div>
  );
}

function DiagnosticsPanel({
  icon,
  title,
  rows,
  footer,
  actions,
}: {
  icon: ReactNode;
  title: string;
  rows: Array<[string, string]>;
  footer: string;
  actions?: ReactNode;
}) {
  return (
    <div className="rounded-xl border border-cyan-500/10 bg-slate-950/40 p-4">
      <div className="mb-4 flex items-center gap-2 text-sm font-semibold text-slate-100">
        {icon}
        {title}
      </div>
      <div className="grid gap-2 sm:grid-cols-2">
        {rows.map(([label, value]) => (
          <div key={label} className="rounded-lg border border-cyan-500/10 bg-slate-950/50 p-3">
            <div className="font-mono text-[9px] uppercase tracking-widest text-slate-500">{label}</div>
            <div className="mt-1 truncate text-sm font-semibold text-slate-100" title={value}>
              {value}
            </div>
          </div>
        ))}
      </div>
      <p className="mt-3 line-clamp-2 text-xs text-slate-500">{footer}</p>
      {actions && <div className="mt-4">{actions}</div>}
    </div>
  );
}

function PowerButton({
  disabled,
  onClick,
  label,
}: {
  disabled: boolean;
  onClick: () => void;
  label: string;
}) {
  return (
    <button
      disabled={disabled}
      onClick={onClick}
      className="rounded-lg border border-cyan-400/30 bg-cyan-400/10 px-3 py-2 text-xs font-medium text-cyan-100 transition-all hover:bg-cyan-400/15 disabled:opacity-50"
    >
      {label}
    </button>
  );
}

function Notice({ message, tone }: { message: string; tone: "danger" | "info" | "warning" }) {
  const toneClass =
    tone === "danger"
      ? "border-rose-400/25 bg-rose-400/10 text-rose-100"
      : tone === "warning"
        ? "border-amber-400/25 bg-amber-400/10 text-amber-100"
        : "border-cyan-400/25 bg-cyan-400/10 text-cyan-100";
  return (
    <div className={`rounded-xl border px-4 py-3 text-sm ${toneClass}`}>
      {message}
    </div>
  );
}

function errorMessage(error: unknown) {
  if (error instanceof Error) return error.message;
  return String(error);
}

function formatMs(value?: number | null) {
  if (value == null || !Number.isFinite(value)) return "--";
  return `${Math.round(value)} ms`;
}

function formatPercent(value?: number | null) {
  if (value == null || !Number.isFinite(value)) return "--";
  return `${Math.round(value)}%`;
}

function formatClock(current?: number | null, max?: number | null) {
  if (current == null && max == null) return "--";
  if (current != null && max != null) return `${Math.round(current)} / ${Math.round(max)} MHz`;
  return `${Math.round(current ?? max ?? 0)} MHz`;
}

function powerSourceLabel(value?: string | null) {
  if (value === "ac") return "tomada";
  if (value === "battery") return "bateria";
  return value ?? "--";
}

function energyPlanLabel(alias?: string | null, fallback?: string | null) {
  if (alias === "high_performance") return "alto desempenho";
  if (alias === "balanced") return "equilibrado";
  if (alias === "power_saver") return "economia";
  return fallback ?? "--";
}

function InventoryPanel({
  title,
  empty,
  items,
  disabled,
}: {
  title: string;
  empty: string;
  items: Array<{
    key: string;
    title: string;
    subtitle: string;
    badge: string;
    action: string;
    restore: string;
    onAction: () => Promise<void>;
    onRestore: () => Promise<void>;
  }>;
  disabled: boolean;
}) {
  return (
    <div className="rounded-xl border border-cyan-500/10 bg-slate-950/40 p-4">
      <div className="mb-3 text-sm font-semibold text-slate-100">{title}</div>
      {items.length === 0 ? (
        <p className="text-xs text-slate-500">{empty}</p>
      ) : (
        <div className="flex flex-col gap-2">
          {items.map((item) => (
            <div key={item.key} className="rounded-lg border border-cyan-500/10 bg-slate-950/50 p-3">
              <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
                <div className="min-w-0">
                  <div className="truncate text-sm font-medium text-slate-100">{item.title}</div>
                  <div className="mt-0.5 truncate font-mono text-[10px] text-slate-500">{item.subtitle}</div>
                  <span className="mt-2 inline-flex rounded-md border border-cyan-500/15 bg-cyan-500/10 px-2 py-0.5 font-mono text-[9px] uppercase tracking-widest text-cyan-200">
                    {item.badge}
                  </span>
                </div>
                <div className="flex shrink-0 gap-2">
                  <button
                    disabled={disabled}
                    onClick={() => void item.onAction()}
                    className="rounded-lg border border-amber-400/30 bg-amber-400/10 px-3 py-2 text-xs font-medium text-amber-100 transition-all hover:bg-amber-400/15 disabled:opacity-50"
                  >
                    {item.action}
                  </button>
                  <button
                    disabled={disabled}
                    onClick={() => void item.onRestore()}
                    className="rounded-lg border border-cyan-400/30 bg-cyan-400/10 px-3 py-2 text-xs font-medium text-cyan-100 transition-all hover:bg-cyan-400/15 disabled:opacity-50"
                  >
                    {item.restore}
                  </button>
                </div>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function HistoryPanel({
  title,
  empty,
  rows,
}: {
  title: string;
  empty: string;
  rows: Array<{ key: string; title: string; detail: string; time: number }>;
}) {
  return (
    <div className="rounded-xl border border-cyan-500/10 bg-slate-950/40 p-4">
      <div className="mb-3 text-sm font-semibold text-slate-100">{title}</div>
      {rows.length === 0 ? (
        <p className="text-xs text-slate-500">{empty}</p>
      ) : (
        <div className="flex flex-col gap-2">
          {rows.map((row) => (
            <article key={row.key} className="rounded-lg border border-cyan-500/10 bg-slate-950/50 p-3">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <div className="truncate text-sm font-medium text-slate-100">{row.title}</div>
                  <div className="mt-0.5 line-clamp-2 text-xs text-slate-500">{row.detail}</div>
                </div>
                <time className="shrink-0 font-mono text-[10px] text-slate-500">{formatEventTime(row.time)}</time>
              </div>
            </article>
          ))}
        </div>
      )}
    </div>
  );
}

function formatEventTime(timestamp: number) {
  if (!Number.isFinite(timestamp) || timestamp <= 0) return "--";
  return new Date(timestamp * 1000).toLocaleString();
}
