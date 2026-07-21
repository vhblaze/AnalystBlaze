import { BatteryCharging, BookOpen, Briefcase, Gamepad2, Gauge, History, ListChecks, Lock, PhoneCall, RefreshCw, Shield, ShieldCheck, Sparkles, Wifi, Wrench } from "lucide-react";
import type { ReactNode } from "react";
import { useEffect, useMemo, useState } from "react";
import { canUseAutomaticGameMode, canUsePaidGameMode } from "@/hooks/useAuth";
import { useI18n } from "@/i18n";
import { useTelemetry } from "@/hooks/useTelemetry";
import {
  addProtectedApp,
  getLocalAiPolicy,
  getAuditLog,
  getActiveFocusSession,
  getEnergyDiagnostics,
  getActiveGameModeSession,
  getNetworkDiagnostics,
  getOptimizationSnapshots,
  getPrivilegedHelperStatus,
  getProtectedApps,
  getWindowsInventory,
  installPrivilegedHelper,
  isTauriRuntime,
  listNetworkAdapters,
  restartPrivilegedHelper,
  runPerformanceScan,
  scanCleanupCategories,
  scanStartupImpact,
  removeProtectedApp,
  uninstallPrivilegedHelper,
  type AuditEvent,
  type AgentStatus,
  type CleanupCategory,
  type EnergyDiagnostics,
  type FocusModeProfile,
  type FocusSession,
  type GameModeSession,
  type LocalAiPolicy,
  type NetworkAdapterSummary,
  type NetworkDiagnostics,
  type OptimizationSnapshot,
  type PerformanceReport,
  type PrivilegedHelperStatus,
  type ProtectedApp,
  type StartupImpact,
  type WindowsInventory,
} from "@/services/tauri/agent";

export function LocalControls({
  status,
  automaticGameModeAllowed,
  busy,
  onActivateGameMode,
  onActivateFocusMode,
  onRestoreOptimizations,
  onDisableStartup,
  onRestoreStartup,
  onStopService,
  onRestoreService,
  onSetPowerPlan,
  onApplyVisualPerformance,
  onRestoreVisualPerformance,
  onDeepCleanTemp,
  onPurgeCleanup,
  onRestoreGameMode,
  onRestoreFocusMode,
  onApplyPcCleanFast,
  onRestorePerformanceSession,
  onApplyCleanupCategory,
  onDelayStartupApp,
  onRestoreDelayedStartupApp,
  onFlushDnsCache,
  onSetDnsServers,
  onResetWinsockCatalog,
  onOpenBilling,
}: {
  status: AgentStatus | null;
  automaticGameModeAllowed?: boolean;
  busy: boolean;
  onActivateGameMode: () => Promise<unknown>;
  onActivateFocusMode: (profile: FocusModeProfile) => Promise<unknown>;
  onRestoreOptimizations: () => Promise<unknown>;
  onDisableStartup: (name: string, location?: string | null) => Promise<unknown>;
  onRestoreStartup: (name?: string | null) => Promise<unknown>;
  onStopService: (name: string) => Promise<unknown>;
  onRestoreService: (name?: string | null) => Promise<unknown>;
  onSetPowerPlan: (plan: "high_performance" | "balanced" | "power_saver") => Promise<unknown>;
  onApplyVisualPerformance: () => Promise<unknown>;
  onRestoreVisualPerformance: () => Promise<unknown>;
  onDeepCleanTemp: () => Promise<unknown>;
  onPurgeCleanup: () => Promise<unknown>;
  onRestoreGameMode: () => Promise<unknown>;
  onRestoreFocusMode: () => Promise<unknown>;
  onApplyPcCleanFast: () => Promise<unknown>;
  onRestorePerformanceSession: (sessionId?: string | null) => Promise<unknown>;
  onApplyCleanupCategory: (category: string, mode?: string | null) => Promise<unknown>;
  onDelayStartupApp: (name: string, location?: string | null) => Promise<unknown>;
  onRestoreDelayedStartupApp: (name?: string | null) => Promise<unknown>;
  onFlushDnsCache: () => Promise<unknown>;
  onSetDnsServers: (adapterName: string, dnsServers: string[]) => Promise<unknown>;
  onResetWinsockCatalog: () => Promise<unknown>;
  onOpenBilling: () => Promise<unknown>;
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
  const [activeGameModeSession, setActiveGameModeSession] = useState<GameModeSession | null>(null);
  const [activeFocusSession, setActiveFocusSession] = useState<FocusSession | null>(status?.focus_session ?? null);
  const [localAiPolicy, setLocalAiPolicy] = useState<LocalAiPolicy | null>(null);
  const [networkDiagnostics, setNetworkDiagnostics] = useState<NetworkDiagnostics | null>(null);
  const [networkAdapters, setNetworkAdapters] = useState<NetworkAdapterSummary[]>([]);
  const [dnsAdapterName, setDnsAdapterName] = useState("");
  const [dnsPrimary, setDnsPrimary] = useState("");
  const [dnsSecondary, setDnsSecondary] = useState("");
  const [energyDiagnostics, setEnergyDiagnostics] = useState<EnergyDiagnostics | null>(null);
  const [performanceReport, setPerformanceReport] = useState<PerformanceReport | null>(null);
  const [cleanupCategories, setCleanupCategories] = useState<CleanupCategory[]>([]);
  const [startupImpact, setStartupImpact] = useState<StartupImpact[]>([]);
  const [diagnosticsBusy, setDiagnosticsBusy] = useState(false);
  const [performanceBusy, setPerformanceBusy] = useState(false);
  const [diagnosticsError, setDiagnosticsError] = useState<string | null>(null);
  const [performanceError, setPerformanceError] = useState<string | null>(null);
  const [inventoryError, setInventoryError] = useState<string | null>(null);
  const [historyError, setHistoryError] = useState<string | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const runtimeAvailable = isTauriRuntime();
  const paidGameModeAllowed = canUsePaidGameMode(status);
  const autoGameModePlanAllowed = automaticGameModeAllowed ?? canUseAutomaticGameMode(status);
  const autoGameModeLabel = automaticGameModeStatusLabel(status, localAiPolicy, autoGameModePlanAllowed);
  const gameModeActive = paidGameModeAllowed && Boolean(activeGameModeSession);
  const focusModeActive = Boolean(activeFocusSession);
  const hasPendingRestore =
    snapshots.some((snapshot) => !snapshot.restored_at) ||
    focusModeActive ||
    performanceReport?.restoreSession?.status === "available";
  const activeProfileLabel = gameModeActive
    ? "Modo Gamer Ativado"
    : performanceReport?.metrics.gameDetected
    ? "Modo Gamer preparado"
    : performanceReport?.mode === "after"
      ? "PC limpo/rapido"
      : "Monitorando";
  const gameModeTargetLabel = activeGameModeSession?.targetProcessName ?? performanceReport?.metrics.gameProcess ?? "Sem jogo detectado";
  const activeFocusLabel = activeFocusSession?.label ?? "Sem foco ativo";

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
    void refreshPerformanceSuite();
  }, []);

  useEffect(() => {
    setActiveFocusSession(status?.focus_session ?? null);
  }, [status?.focus_session]);

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
      const [nextAudit, nextSnapshots, nextProtected, nextHelper, nextGameModeSession, nextFocusSession] = await Promise.all([
        getAuditLog(120),
        getOptimizationSnapshots(120),
        getProtectedApps(),
        getPrivilegedHelperStatus(),
        getActiveGameModeSession(),
        getActiveFocusSession(),
      ]);
      const nextPolicy = await getLocalAiPolicy();
      setAuditEvents(nextAudit);
      setSnapshots(nextSnapshots);
      setProtectedApps(nextProtected);
      setHelperStatus(nextHelper);
      setActiveGameModeSession(nextGameModeSession);
      setActiveFocusSession(nextFocusSession);
      setLocalAiPolicy(nextPolicy);
      track("local_history_refreshed");
    } catch (error) {
      setAuditEvents([]);
      setSnapshots([]);
      setProtectedApps([]);
      setHelperStatus(null);
      setActiveGameModeSession(null);
      setActiveFocusSession(null);
      setLocalAiPolicy(null);
      setHistoryError(errorMessage(error));
    }
  };

  const refreshNetworkAndEnergy = async () => {
    setDiagnosticsBusy(true);
    setDiagnosticsError(null);
    try {
      const [nextNetwork, nextEnergy, nextAdapters] = await Promise.all([
        getNetworkDiagnostics(),
        getEnergyDiagnostics(),
        listNetworkAdapters(),
      ]);
      setNetworkDiagnostics(nextNetwork);
      setEnergyDiagnostics(nextEnergy);
      setNetworkAdapters(nextAdapters);
      setDnsAdapterName((current) => {
        if (current && nextAdapters.some((adapter) => adapter.name === current)) return current;
        return nextNetwork.adapter_name ?? nextAdapters[0]?.name ?? "";
      });
      track("network_energy_refreshed");
    } catch (error) {
      setNetworkDiagnostics(null);
      setEnergyDiagnostics(null);
      setNetworkAdapters([]);
      setDiagnosticsError(errorMessage(error));
    } finally {
      setDiagnosticsBusy(false);
    }
  };

  const refreshPerformanceSuite = async (mode: "baseline" | "after" | "quick" = "quick") => {
    setPerformanceBusy(true);
    setPerformanceError(null);
    try {
      const [report, categories, startup] = await Promise.all([
        runPerformanceScan(mode),
        scanCleanupCategories(),
        scanStartupImpact(),
      ]);
      setPerformanceReport(report);
      setCleanupCategories(categories);
      setStartupImpact(startup);
      track("performance_suite_refreshed", { mode, score: report.overallScore });
    } catch (error) {
      setPerformanceReport(null);
      setCleanupCategories([]);
      setStartupImpact([]);
      setPerformanceError(errorMessage(error));
    } finally {
      setPerformanceBusy(false);
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

  const runHelperAction = async (
    action: () => Promise<PrivilegedHelperStatus>,
    successMessage: string,
  ) => {
    await runControlAction(
      async () => {
        const nextStatus = await action();
        setHelperStatus(nextStatus);
        await refreshOperationalHistory();
      },
      successMessage,
    );
  };

  const runTempAction = async (action: () => Promise<unknown>, successMessage: string) => {
    await runControlAction(
      async () => {
        const result = await action();
        if (result === false) return false;
        await refreshOperationalHistory();
      },
      successMessage,
    );
  };

  const runVisualAction = async (action: () => Promise<unknown>, successMessage: string) => {
    await runControlAction(
      async () => {
        const result = await action();
        if (result === false) return false;
        await refreshOperationalHistory();
      },
      successMessage,
    );
  };

  const runNetworkAction = async (action: () => Promise<unknown>, successMessage: string) => {
    await runControlAction(
      async () => {
        const result = await action();
        if (result === false) return false;
        await refreshNetworkAndEnergy();
        await refreshOperationalHistory();
      },
      successMessage,
    );
  };

  const applyDnsServers = async () => {
    const servers = [dnsPrimary, dnsSecondary].map((value) => value.trim()).filter(Boolean);
    if (!dnsAdapterName || servers.length === 0) {
      setActionMessage("Selecione um adaptador e informe ao menos um servidor DNS.");
      return;
    }
    await runNetworkAction(
      () => onSetDnsServers(dnsAdapterName, servers),
      "Servidores DNS alterados.",
    );
  };

  const runPerformanceAction = async (action: () => Promise<unknown>, successMessage: string) => {
    await runControlAction(
      async () => {
        const result = await action();
        if (result === false) return false;
        await refreshPerformanceSuite("after");
        await refreshOperationalHistory();
        await refreshWindowsInventory();
      },
      successMessage,
    );
  };

  const handleGameModeUpsellClick = async () => {
    track("game_mode_upsell_clicked");
    await onOpenBilling();
  };

  const deactivateGameMode = async () => {
    await runControlAction(
      async () => {
        const result = await onRestoreGameMode();
        if (result === false) return false;
        setActiveGameModeSession(null);
        await refreshOperationalHistory();
        await refreshNetworkAndEnergy();
        await refreshPerformanceSuite("after");
      },
      "Modo Gamer desativado.",
    );
  };

  const activateFocus = async (profile: FocusModeProfile) => {
    await runControlAction(
      async () => {
        const result = await onActivateFocusMode(profile);
        if (result === false) return false;
        const nextFocusSession = await getActiveFocusSession();
        setActiveFocusSession(nextFocusSession);
        await refreshOperationalHistory();
      },
      "Modo Foco ativado.",
    );
  };

  const deactivateFocus = async () => {
    await runControlAction(
      async () => {
        const result = await onRestoreFocusMode();
        if (result === false) return false;
        setActiveFocusSession(null);
        await refreshOperationalHistory();
      },
      "Modo Foco restaurado.",
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
        <div className="flex flex-col gap-5 lg:flex-row lg:items-start lg:justify-between">
          <div className="max-w-2xl">
            <div className="flex items-center gap-2 font-mono text-[11px] uppercase tracking-[0.25em] text-cyan-400/80">
              <Gamepad2 className="h-3.5 w-3.5 text-cyan-300" />
              Perfil principal
            </div>
            <h2 className="mt-3 text-2xl font-semibold tracking-tight text-slate-50">
              Modo Gamer completo
            </h2>
            <p className="mt-2 text-sm leading-relaxed text-slate-400">
              Um clique prioriza o jogo ou app ativo, reduz interferencia de fundo, aplica limpeza segura, ajusta energia/visual e deixa a restauracao pronta.
            </p>
          </div>
          <div className="flex flex-wrap gap-2 lg:justify-end">
            <div className="flex flex-col items-stretch gap-1">
              <button
                disabled={busy || performanceBusy || !runtimeAvailable || gameModeActive}
                onClick={() =>
                  paidGameModeAllowed
                    ? void runPerformanceAction(onActivateGameMode, "Modo Gamer completo aplicado.")
                    : void handleGameModeUpsellClick()
                }
                className={`inline-flex items-center justify-center gap-2 rounded-xl border px-4 py-2.5 text-sm font-semibold transition-all disabled:opacity-70 ${
                  gameModeActive
                    ? "border-emerald-300/55 bg-emerald-400/15 text-emerald-50"
                    : "border-cyan-300/50 bg-cyan-400/15 text-cyan-50 hover:bg-cyan-400/20"
                }`}
              >
                {gameModeActive ? <ShieldCheck className="h-4 w-4" /> : paidGameModeAllowed ? <Gamepad2 className="h-4 w-4" /> : <Lock className="h-4 w-4" />}
                {gameModeActive ? "Modo Gamer Ativado" : paidGameModeAllowed ? "Ativar Modo Gamer" : "Desbloquear Modo Gamer"}
              </button>
              {gameModeActive && (
                <button
                  disabled={busy || performanceBusy || !runtimeAvailable}
                  onClick={() => void deactivateGameMode()}
                  className="text-left text-xs font-medium text-amber-200 transition hover:text-amber-100 disabled:opacity-50"
                >
                  Desativar
                </button>
              )}
              {!paidGameModeAllowed && (
                <span className="text-xs text-slate-500">Abre a pagina de planos</span>
              )}
            </div>
            <button
              disabled={busy || performanceBusy || !runtimeAvailable}
              onClick={() => void runPerformanceAction(onApplyPcCleanFast, "Perfil PC limpo/rapido aplicado.")}
              className="inline-flex items-center gap-2 rounded-xl border border-emerald-400/40 bg-emerald-400/10 px-4 py-2.5 text-sm font-semibold text-emerald-100 transition-all hover:bg-emerald-400/15 disabled:opacity-50"
            >
              <Sparkles className="h-4 w-4" />
              PC limpo/rapido
            </button>
            <button
              disabled={busy || !runtimeAvailable}
              onClick={() => void runControlAction(
                async () => {
                  const result = await onRestoreOptimizations();
                  if (result === false) return false;
                  await refreshOperationalHistory();
                  await refreshWindowsInventory();
                  await refreshPerformanceSuite("after");
                },
                t("controls.restoreCompleted"),
              )}
              className="inline-flex items-center gap-2 rounded-xl border border-amber-400/40 bg-amber-400/10 px-4 py-2.5 text-sm font-semibold text-amber-100 transition-all hover:bg-amber-400/15 disabled:opacity-50"
            >
              <ShieldCheck className="h-4 w-4" />
              Restaurar alteracoes
            </button>
            <button
              disabled={performanceBusy}
              onClick={() => void refreshPerformanceSuite("quick")}
              className="inline-flex items-center gap-2 rounded-xl border border-cyan-400/35 bg-cyan-400/10 px-4 py-2.5 text-sm font-medium text-cyan-100 transition-all hover:bg-cyan-400/15 disabled:opacity-50"
            >
              <RefreshCw className={`h-4 w-4 ${performanceBusy ? "animate-spin" : ""}`} />
              Recalcular score
            </button>
          </div>
        </div>

        {performanceError && <Notice tone="danger" message={performanceError} />}
        <div className="mt-5 grid gap-3 sm:grid-cols-2 xl:grid-cols-5">
          <SummaryTile label="Score atual" value={performanceReport ? `${Math.round(performanceReport.overallScore)}/100` : "--"} detail={performanceReport?.bottlenecks[0]?.label ?? "Sem scan recente"} />
          <SummaryTile label="Resultado medido" value={formatMeasuredChange(performanceReport)} detail="Comparado ao baseline" />
          <SummaryTile label="Perfil" value={activeProfileLabel} detail={gameModeTargetLabel} />
          <SummaryTile label="Automacao" value={autoGameModeLabel} detail={autoGameModePlanAllowed ? "Pro/Family" : "Starter manual"} />
          <SummaryTile label="Restauracao" value={hasPendingRestore ? "Disponivel" : "Limpa"} detail={helperStatus?.running ? "Helper rodando" : "Sem helper ativo"} />
        </div>

        <div className="mt-5 flex flex-col gap-3 border-t border-cyan-500/10 pt-5 lg:flex-row lg:items-center lg:justify-between">
          <div className="min-w-0">
            <div className="font-mono text-[10px] uppercase tracking-[0.24em] text-cyan-400/70">
              Modo Foco
            </div>
            <div className="mt-1 truncate text-sm font-semibold text-slate-100" title={activeFocusLabel}>
              {focusModeActive ? activeFocusLabel : "Trabalho, jogo, chamada ou estudo"}
            </div>
          </div>
          <div className="flex flex-wrap gap-2">
            <button
              disabled={busy || !runtimeAvailable}
              onClick={() => void activateFocus("work")}
              className="inline-flex items-center gap-2 rounded-xl border border-sky-400/35 bg-sky-400/10 px-3 py-2 text-xs font-semibold text-sky-100 transition hover:bg-sky-400/15 disabled:opacity-50"
            >
              <Briefcase className="h-3.5 w-3.5" />
              Trabalho
            </button>
            <button
              disabled={busy || !runtimeAvailable}
              onClick={() => void activateFocus("game")}
              className="inline-flex items-center gap-2 rounded-xl border border-cyan-400/35 bg-cyan-400/10 px-3 py-2 text-xs font-semibold text-cyan-100 transition hover:bg-cyan-400/15 disabled:opacity-50"
            >
              <Gamepad2 className="h-3.5 w-3.5" />
              Jogo
            </button>
            <button
              disabled={busy || !runtimeAvailable}
              onClick={() => void activateFocus("call")}
              className="inline-flex items-center gap-2 rounded-xl border border-emerald-400/35 bg-emerald-400/10 px-3 py-2 text-xs font-semibold text-emerald-100 transition hover:bg-emerald-400/15 disabled:opacity-50"
            >
              <PhoneCall className="h-3.5 w-3.5" />
              Chamada
            </button>
            <button
              disabled={busy || !runtimeAvailable}
              onClick={() => void activateFocus("study")}
              className="inline-flex items-center gap-2 rounded-xl border border-violet-400/35 bg-violet-400/10 px-3 py-2 text-xs font-semibold text-violet-100 transition hover:bg-violet-400/15 disabled:opacity-50"
            >
              <BookOpen className="h-3.5 w-3.5" />
              Estudo
            </button>
            {focusModeActive && (
              <button
                disabled={busy || !runtimeAvailable}
                onClick={() => void deactivateFocus()}
                className="inline-flex items-center gap-2 rounded-xl border border-amber-400/35 bg-amber-400/10 px-3 py-2 text-xs font-semibold text-amber-100 transition hover:bg-amber-400/15 disabled:opacity-50"
              >
                <ShieldCheck className="h-3.5 w-3.5" />
                Restaurar
              </button>
            )}
          </div>
        </div>
      </section>

      <AdvancedSection
        icon={<Wrench className="h-4 w-4 text-cyan-300" />}
        title="Avancado: comandos e diagnosticos"
        description="Controles tecnicos, categorias detalhadas, helper admin, apps protegidos e historico ficam recolhidos para nao poluir o fluxo principal."
      >
      <section className="glass-panel cyber-glow p-6">
        <div className="flex flex-col gap-3 pb-4 md:flex-row md:items-center md:justify-between">
          <div className="flex items-center gap-2">
            <Gauge className="h-3.5 w-3.5 text-cyan-300" />
            <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-cyan-400/80">Performance Suite</h2>
          </div>
          <div className="flex flex-wrap gap-2">
            <button
              disabled={performanceBusy}
              onClick={() => void refreshPerformanceSuite("baseline")}
              className="inline-flex items-center gap-2 rounded-xl border border-cyan-400/40 bg-cyan-400/10 px-3 py-2 text-xs font-medium text-cyan-100 transition-all hover:border-cyan-300/60 disabled:opacity-50"
            >
              <RefreshCw className={`h-3.5 w-3.5 ${performanceBusy ? "animate-spin" : ""}`} />
              Baseline
            </button>
            <button
              disabled={performanceBusy}
              onClick={() => void refreshPerformanceSuite("after")}
              className="inline-flex items-center gap-2 rounded-xl border border-cyan-400/40 bg-cyan-400/10 px-3 py-2 text-xs font-medium text-cyan-100 transition-all hover:border-cyan-300/60 disabled:opacity-50"
            >
              Depois
            </button>
            <button
              disabled={busy || performanceBusy || !runtimeAvailable}
              onClick={() => void runPerformanceAction(onApplyPcCleanFast, "Perfil PC limpo/rapido aplicado.")}
              className="inline-flex items-center gap-2 rounded-xl border border-emerald-400/40 bg-emerald-400/10 px-3 py-2 text-xs font-semibold text-emerald-100 transition-all hover:border-emerald-300/60 disabled:opacity-50"
            >
              <Sparkles className="h-3.5 w-3.5" />
              Aplicar PC limpo/rapido
            </button>
            <button
              disabled={busy || performanceBusy || !runtimeAvailable}
              onClick={() => void runPerformanceAction(() => onRestorePerformanceSession(performanceReport?.restoreSession?.id), "Sessao de performance restaurada.")}
              className="inline-flex items-center gap-2 rounded-xl border border-amber-400/40 bg-amber-400/10 px-3 py-2 text-xs font-medium text-amber-100 transition-all hover:border-amber-300/60 disabled:opacity-50"
            >
              Restaurar suite
            </button>
          </div>
        </div>
        {performanceError && <Notice tone="danger" message={performanceError} />}
        <div className="grid gap-4 lg:grid-cols-4">
          <DiagnosticsPanel
            icon={<Gauge className="h-4 w-4 text-cyan-300" />}
            title="Score medido"
            rows={[
              ["Atual", performanceReport ? `${Math.round(performanceReport.overallScore)}/100` : "--"],
              ["Mudanca", formatMeasuredChange(performanceReport)],
              ["Modo", performanceReport?.mode ?? "--"],
              ["Jogo", performanceReport?.metrics.gameDetected ? performanceReport.metrics.gameProcess ?? "detectado" : "nao detectado"],
            ]}
            footer={performanceReport?.bottlenecks[0]?.label ?? "Rode um baseline para medir responsividade local."}
          />
          <DiagnosticsPanel
            icon={<Sparkles className="h-4 w-4 text-cyan-300" />}
            title="Gargalos"
            rows={[
              ["Startup", formatScore(performanceReport?.scoreBreakdown.bootStartup)],
              ["Fundo", formatScore(performanceReport?.scoreBreakdown.background)],
              ["RAM", formatScore(performanceReport?.scoreBreakdown.memory)],
              ["Disco", formatScore(performanceReport?.scoreBreakdown.disk)],
              ["Rede", formatScore(performanceReport?.scoreBreakdown.network)],
              ["Termal", formatScore(performanceReport?.scoreBreakdown.thermal)],
            ]}
            footer={performanceReport?.bottlenecks.map((item) => item.label).join(" / ") || "Sem gargalo critico no ultimo scan."}
          />
          <DiagnosticsPanel
            icon={<ListChecks className="h-4 w-4 text-cyan-300" />}
            title="Limpeza elegivel"
            rows={cleanupCategories.slice(0, 6).map((category) => [
              category.label,
              `${formatBytes(category.reclaimableBytes)}${category.requiresHelper ? " / helper" : ""}`,
            ])}
            footer="Categorias usam quarentena reversivel; purge libera espaco real depois."
          />
          <DiagnosticsPanel
            icon={<History className="h-4 w-4 text-cyan-300" />}
            title="Inicializacao"
            rows={startupImpact.slice(0, 6).map((app) => [
              app.name,
              `${Math.round(app.impactScore)} - ${app.recommendation}`,
            ])}
            footer="Apps seguros podem ser atrasados com snapshot local."
          />
        </div>
        <div className="mt-4 grid gap-4 lg:grid-cols-2">
          <ActionList
            title="Categorias de limpeza"
            empty="Nenhuma categoria elegivel encontrada."
            items={cleanupCategories.slice(0, 6).map((category) => ({
              key: category.id,
              title: category.label,
              detail: `${formatBytes(category.reclaimableBytes)} - risco ${category.risk}`,
              action: category.id === "cleanup_quarantine" ? "Purgar" : "Aplicar",
              onAction: () => runPerformanceAction(
                () => onApplyCleanupCategory(category.id, category.id === "cleanup_quarantine" ? "purge" : "safe"),
                "Categoria processada.",
              ),
            }))}
            disabled={busy || performanceBusy}
          />
          <ActionList
            title="Apps de inicializacao"
            empty="Nenhum app de alto impacto seguro foi encontrado."
            items={startupImpact.filter((app) => app.availableActions.includes("delay")).slice(0, 6).map((app) => ({
              key: `${app.location}:${app.name}`,
              title: app.name,
              detail: `${Math.round(app.impactScore)} pontos - ${app.location}`,
              action: "Atrasar",
              secondaryAction: "Restaurar",
              onAction: () => runPerformanceAction(
                () => onDelayStartupApp(app.name, app.location),
                "App movido para inicializacao atrasada.",
              ),
              onSecondaryAction: () => runPerformanceAction(
                () => onRestoreDelayedStartupApp(app.name),
                "App restaurado na inicializacao.",
              ),
            }))}
            disabled={busy || performanceBusy}
          />
        </div>
      </section>

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
                <PowerButton
                  disabled={busy || diagnosticsBusy}
                  onClick={() => void runVisualAction(onApplyVisualPerformance, "Efeitos visuais ajustados para desempenho.")}
                  label="Visual desempenho"
                />
                <PowerButton
                  disabled={busy || diagnosticsBusy}
                  onClick={() => void runVisualAction(onRestoreVisualPerformance, "Efeitos visuais restaurados.")}
                  label="Restaurar visual"
                />
              </div>
            }
          />
        </div>

        <div className="mt-4 rounded-xl border border-cyan-500/10 bg-slate-950/40 p-4">
          <div className="flex items-center gap-2 text-sm font-semibold text-slate-100">
            <Wifi className="h-4 w-4 text-cyan-300" />
            Ajustes de rede admin
          </div>
          <p className="mt-1 text-xs text-slate-500">
            Acoes que mexem em DNS e no catalogo Winsock. Troca de DNS e reset de Winsock exigem o helper privilegiado instalado.
          </p>
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <button
              disabled={busy || diagnosticsBusy || !runtimeAvailable}
              onClick={() => void runNetworkAction(onFlushDnsCache, "Cache de DNS limpo.")}
              className="rounded-lg border border-cyan-400/30 bg-cyan-400/10 px-3 py-2 text-xs font-medium text-cyan-100 transition hover:bg-cyan-400/15 disabled:opacity-50"
            >
              Limpar cache DNS
            </button>
            <button
              disabled={busy || diagnosticsBusy || !runtimeAvailable || !helperStatus?.available}
              onClick={() => void runNetworkAction(
                onResetWinsockCatalog,
                "Catalogo Winsock resetado. Reinicie o computador para concluir.",
              )}
              className="rounded-lg border border-rose-400/30 bg-rose-400/10 px-3 py-2 text-xs font-medium text-rose-100 transition hover:bg-rose-400/15 disabled:opacity-50"
              title={!helperStatus?.available ? "Instale o helper privilegiado para liberar esta acao." : "Exige reinicializacao do computador."}
            >
              Resetar Winsock
            </button>
          </div>

          <div className="mt-4 grid gap-2 sm:grid-cols-4">
            <select
              value={dnsAdapterName}
              onChange={(event) => setDnsAdapterName(event.target.value)}
              disabled={busy || diagnosticsBusy || !runtimeAvailable || networkAdapters.length === 0}
              className="min-h-11 rounded-xl border border-cyan-500/20 bg-slate-950/60 px-3 text-sm text-slate-100 outline-none transition focus:border-cyan-300/60 sm:col-span-2 disabled:opacity-50"
            >
              {networkAdapters.length === 0 ? (
                <option value="">Nenhum adaptador ativo encontrado</option>
              ) : (
                networkAdapters.map((adapter) => (
                  <option key={adapter.name} value={adapter.name}>
                    {adapter.name}
                  </option>
                ))
              )}
            </select>
            <input
              value={dnsPrimary}
              onChange={(event) => setDnsPrimary(event.target.value)}
              placeholder="DNS primario (ex: 1.1.1.1)"
              className="min-h-11 rounded-xl border border-cyan-500/20 bg-slate-950/60 px-3 text-sm text-slate-100 outline-none transition focus:border-cyan-300/60"
            />
            <input
              value={dnsSecondary}
              onChange={(event) => setDnsSecondary(event.target.value)}
              placeholder="DNS secundario (opcional)"
              className="min-h-11 rounded-xl border border-cyan-500/20 bg-slate-950/60 px-3 text-sm text-slate-100 outline-none transition focus:border-cyan-300/60"
            />
          </div>
          <button
            disabled={busy || diagnosticsBusy || !runtimeAvailable || !helperStatus?.available || !dnsAdapterName}
            onClick={() => void applyDnsServers()}
            className="mt-3 inline-flex items-center gap-2 rounded-xl border border-cyan-400/40 bg-cyan-400/10 px-4 py-2.5 text-sm font-semibold text-cyan-100 transition-all hover:bg-cyan-400/15 disabled:opacity-50"
            title={!helperStatus?.available ? "Instale o helper privilegiado para liberar esta acao." : undefined}
          >
            Aplicar DNS
          </button>
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
            <button
              disabled={busy}
              onClick={() => void runControlAction(
                async () => {
                  const result = await onRestoreGameMode();
                  if (result === false) return false;
                  setActiveGameModeSession(null);
                  await refreshOperationalHistory();
                  await refreshNetworkAndEnergy();
                },
                "Modo Gamer restaurado.",
              )}
              className="inline-flex items-center gap-2 rounded-xl border border-amber-400/40 bg-amber-400/10 px-3 py-2 text-xs font-medium text-amber-100 transition-all hover:border-amber-300/60 disabled:opacity-50"
            >
              <ShieldCheck className="h-3.5 w-3.5" />
              Restaurar modo gamer agora
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
          <div className="mt-3 grid gap-2 text-xs text-slate-400 sm:grid-cols-3">
            <span className="rounded-lg border border-cyan-500/10 bg-slate-950/50 px-3 py-2">
              instalado: {helperStatus?.installed ? "sim" : "nao"}
            </span>
            <span className="rounded-lg border border-cyan-500/10 bg-slate-950/50 px-3 py-2">
              rodando: {helperStatus?.running ? "sim" : "nao"}
            </span>
            <span className="rounded-lg border border-cyan-500/10 bg-slate-950/50 px-3 py-2">
              versao: {helperStatus?.version ?? "--"}
            </span>
          </div>
          <div className="mt-4 flex flex-wrap gap-2">
            <button
              disabled={busy || !runtimeAvailable || helperStatus?.available === true || helperStatus?.canRequestUac === false}
              onClick={() => void runHelperAction(installPrivilegedHelper, "Helper privilegiado instalado.")}
              className="rounded-lg border border-emerald-400/30 bg-emerald-400/10 px-3 py-2 text-xs font-medium text-emerald-100 transition hover:bg-emerald-400/15 disabled:opacity-50"
            >
              Instalar helper
            </button>
            <button
              disabled={busy || !runtimeAvailable || !helperStatus?.installed}
              onClick={() => void runHelperAction(restartPrivilegedHelper, "Helper privilegiado reiniciado.")}
              className="rounded-lg border border-cyan-400/30 bg-cyan-400/10 px-3 py-2 text-xs font-medium text-cyan-100 transition hover:bg-cyan-400/15 disabled:opacity-50"
            >
              Reiniciar helper
            </button>
            <button
              disabled={busy || !runtimeAvailable || !helperStatus?.installed}
              onClick={() => void runHelperAction(uninstallPrivilegedHelper, "Helper privilegiado removido.")}
              className="rounded-lg border border-rose-400/30 bg-rose-400/10 px-3 py-2 text-xs font-medium text-rose-100 transition hover:bg-rose-400/15 disabled:opacity-50"
            >
              Remover helper
            </button>
            <button
              disabled={busy || !runtimeAvailable}
              onClick={() => void runTempAction(onDeepCleanTemp, "Limpeza profunda TEMP concluida.")}
              className="rounded-lg border border-amber-400/30 bg-amber-400/10 px-3 py-2 text-xs font-medium text-amber-100 transition hover:bg-amber-400/15 disabled:opacity-50"
            >
              Limpeza profunda TEMP
            </button>
            <button
              disabled={busy || !runtimeAvailable}
              onClick={() => void runTempAction(onPurgeCleanup, "Quarentena de limpeza apagada.")}
              className="rounded-lg border border-amber-400/30 bg-amber-400/10 px-3 py-2 text-xs font-medium text-amber-100 transition hover:bg-amber-400/15 disabled:opacity-50"
            >
              Purgar quarentena
            </button>
          </div>
        </div>
      </section>
      </AdvancedSection>
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

function SummaryTile({ label, value, detail }: { label: string; value: string; detail: string }) {
  return (
    <div className="min-w-0 rounded-xl border border-cyan-500/10 bg-slate-950/45 p-4">
      <div className="font-mono text-[9px] uppercase tracking-widest text-slate-500">{label}</div>
      <div className="mt-2 truncate text-lg font-semibold text-slate-50" title={value}>
        {value}
      </div>
      <div className="mt-1 truncate text-xs text-slate-500" title={detail}>
        {detail}
      </div>
    </div>
  );
}

function AdvancedSection({
  icon,
  title,
  description,
  children,
}: {
  icon: ReactNode;
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <details className="group rounded-2xl border border-cyan-500/15 bg-slate-950/30">
      <summary className="flex cursor-pointer list-none flex-col gap-3 rounded-2xl px-5 py-4 transition hover:bg-cyan-400/5 md:flex-row md:items-center md:justify-between">
        <span className="flex min-w-0 items-start gap-3">
          <span className="mt-0.5 grid h-8 w-8 shrink-0 place-items-center rounded-lg border border-cyan-500/20 bg-cyan-400/10">
            {icon}
          </span>
          <span className="min-w-0">
            <span className="block text-sm font-semibold text-slate-100">{title}</span>
            <span className="mt-1 block text-xs leading-relaxed text-slate-500">{description}</span>
          </span>
        </span>
        <span className="font-mono text-[10px] uppercase tracking-widest text-cyan-300 group-open:hidden">
          abrir
        </span>
        <span className="hidden font-mono text-[10px] uppercase tracking-widest text-cyan-300 group-open:block">
          recolher
        </span>
      </summary>
      <div className="flex flex-col gap-6 border-t border-cyan-500/10 p-4 md:p-6">
        {children}
      </div>
    </details>
  );
}

function automaticGameModeStatusLabel(
  status: AgentStatus | null,
  policy: LocalAiPolicy | null,
  planAllowed: boolean,
) {
  if (!status?.authenticated || !status.registered) return "Aguardando agente";
  if (!planAllowed) return "Bloqueado pelo plano";
  if (!policy) return "Disponivel no plano";
  if (policy.enabled && policy.auto_game_mode) return "Automatico ativo";
  return "Pausado pela policy";
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

function formatScore(value?: number | null) {
  if (value == null || !Number.isFinite(value)) return "--";
  return `${Math.round(value)}/100`;
}

function formatMeasuredChange(report?: PerformanceReport | null) {
  if (!report || report.previousScore == null || report.scoreDeltaPercent == null || !Number.isFinite(report.scoreDeltaPercent)) {
    return "--";
  }
  const change = report.performanceChange;
  if (change === "stable") return "estavel";
  if (change === "regressed") return `${report.scoreDeltaPercent.toFixed(1)}% queda`;
  const gain = report.measuredGainPercent ?? Math.max(report.scoreDeltaPercent, 0);
  if (!Number.isFinite(gain) || gain <= 0) return "0.0%";
  return `+${gain.toFixed(1)}%`;
}

function formatBytes(bytes?: number | null) {
  if (bytes == null || !Number.isFinite(bytes) || bytes <= 0) return "0 MB";
  const mb = bytes / 1024 / 1024;
  if (mb < 1024) return `${Math.round(mb)} MB`;
  return `${(mb / 1024).toFixed(1)} GB`;
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

function ActionList({
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
    detail: string;
    action: string;
    secondaryAction?: string;
    onAction: () => Promise<void>;
    onSecondaryAction?: () => Promise<void>;
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
              <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
                <div className="min-w-0">
                  <div className="truncate text-sm font-medium text-slate-100">{item.title}</div>
                  <div className="mt-0.5 truncate text-xs text-slate-500">{item.detail}</div>
                </div>
                <div className="flex shrink-0 gap-2">
                  <button
                    disabled={disabled}
                    onClick={() => void item.onAction()}
                    className="rounded-lg border border-emerald-400/30 bg-emerald-400/10 px-3 py-2 text-xs font-medium text-emerald-100 transition-all hover:bg-emerald-400/15 disabled:opacity-50"
                  >
                    {item.action}
                  </button>
                  {item.secondaryAction && item.onSecondaryAction && (
                    <button
                      disabled={disabled}
                      onClick={() => void item.onSecondaryAction?.()}
                      className="rounded-lg border border-cyan-400/30 bg-cyan-400/10 px-3 py-2 text-xs font-medium text-cyan-100 transition-all hover:bg-cyan-400/15 disabled:opacity-50"
                    >
                      {item.secondaryAction}
                    </button>
                  )}
                </div>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
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
