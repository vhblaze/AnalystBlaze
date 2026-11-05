import { Suspense, lazy, useCallback, useState } from "react";
import { ShieldCheck } from "lucide-react";
import { canUseAutomaticGameMode, type useAuth } from "@/hooks/useAuth";
import type { useAgentTelemetry } from "@/hooks/useAgentTelemetry";
import { useI18n } from "@/i18n";
import { fromLegacyView, route, type LegacyView, type Route } from "@/lib/routes";
import type { DiskNearFullInfo } from "@/services/insights";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { History } from "@/components/screens/history/History";
import { auditLog, optimizationSnapshots } from "@/features/history/useHistory";
import { releaseMemory } from "@/services/tauri/agent";
import { MyPC } from "@/components/screens/mypc/MyPC";
import { WindowsHealth } from "@/components/screens/mypc/WindowsHealth";
import type { RequestConfirmation, RunConfirmed } from "./useConfirmation";
import type { ControlsActions } from "@/components/screens/controls/actions";

const Dashboard = lazy(() => import("@/components/screens/dashboard/Dashboard").then((module) => ({ default: module.Dashboard })));
const Telemetry = lazy(() => import("@/components/screens/mypc/Performance").then((module) => ({ default: module.Performance })));
const Insights = lazy(() => import("@/components/analystblaze/views/Insights").then((module) => ({ default: module.Insights })));
const Controls = lazy(() => import("@/components/screens/controls/Controls").then((module) => ({ default: module.Controls })));
const DiskExplorer = lazy(() => import("@/components/screens/mypc/Disk").then((module) => ({ default: module.Disk })));
const Network = lazy(() => import("@/components/screens/mypc/Network").then((module) => ({ default: module.Network })));
const Settings = lazy(() => import("@/components/screens/settings/Settings").then((module) => ({ default: module.Settings })));

function ViewFallback() {
  return <div className="h-44 animate-pulse rounded-card border border-line bg-surface" />;
}

/**
 * O "outlet" do AppShell: escolhe a tela pela rota e liga cada view atual aos
 * comandos do agente, com a mesma confirmação de antes. Etapa 1 do doc 15:
 * as views não mudaram; só passaram a morar na casca nova.
 */
export function ScreenOutlet({
  current,
  navigate,
  auth,
  telemetry,
  runConfirmed,
  requestConfirmation,
  shadowConsentNeeded,
  resolveShadowConsent,
}: {
  current: Route;
  navigate: (next: Route) => void;
  auth: ReturnType<typeof useAuth>;
  telemetry: ReturnType<typeof useAgentTelemetry>;
  runConfirmed: RunConfirmed;
  requestConfirmation: RequestConfirmation;
  shadowConsentNeeded: boolean;
  resolveShadowConsent: (choice: "auto" | "manual") => Promise<void>;
}) {
  const { t } = useI18n();
  const [focusDiskUsage, setFocusDiskUsage] = useState(false);
  const [focusTracerouteTarget, setFocusTracerouteTarget] = useState<string | null>(null);
  const [diskNearFullInfo, setDiskNearFullInfo] = useState<DiskNearFullInfo | null>(null);

  const handleViewChange = useCallback((view: LegacyView) => navigate(fromLegacyView(view)), [navigate]);

  const openDiskUsageDetails = useCallback(() => {
    setFocusDiskUsage(true);
    navigate(route("mypc", "disk"));
  }, [navigate]);

  const openNetworkDetails = useCallback(
    (autoTracerouteTarget?: string) => {
      if (autoTracerouteTarget) setFocusTracerouteTarget(autoTracerouteTarget);
      navigate(route("mypc", "network"));
    },
    [navigate],
  );

  // Starter plan gets a real 1h/week Game Mode budget now (enforced
  // server-side, see activate_game_mode in lib.rs) instead of an outright
  // block - only redirect to billing once that budget is actually
  // exhausted, not pre-emptively.
  const activateGameModeWithUpsell = useCallback(async () => {
    const result = await auth.activateGameMode();
    if (result?.blockedReason === "weekly_limit_reached") {
      await auth.openBilling();
    }
  }, [auth]);

  // "I'll do it myself" from an Insights card - runs the same action right
  // now with the same confirmation dialog its own dedicated button uses,
  // rather than a separate/different flow just because it started from a
  // recommendation instead of a button.
  const applyInsightActionLocally = useCallback(
    (actionName: string, context?: Record<string, unknown>) => {
      if (actionName === "APPLY_GAME_MODE") {
        return runConfirmed(
          {
            title: t("confirm.gameMode.title"),
            description: t("confirm.gameMode.body"),
            risk: "sensivel",
            snapshot: true,
          },
          activateGameModeWithUpsell,
        );
      }
      if (actionName === "EMPTY_TEMP") {
        return runConfirmed(
          {
            title: t("confirm.deepTemp.title"),
            description: t("confirm.deepTemp.body"),
            risk: "sensivel",
            snapshot: true,
          },
          auth.cleanTempDeep,
        );
      }
      if (actionName === "ENABLE_SCHEDULED_DEFRAG") {
        return runConfirmed(
          {
            title: t("confirm.enableDefrag.title"),
            description: t("confirm.enableDefrag.body"),
            risk: "sensivel",
            snapshot: false,
          },
          auth.enableScheduledDefrag,
        );
      }
      if (actionName === "START_SYSTEM_FILE_CHECK") {
        return runConfirmed(
          {
            title: t("confirm.sfc.title"),
            description: t("confirm.sfc.body"),
            risk: "sensivel",
            snapshot: false,
          },
          auth.startSystemFileCheck,
        );
      }
      if (actionName === "RESTART_PNP_DEVICE") {
        const deviceId = typeof context?.deviceId === "string" ? context.deviceId : null;
        if (!deviceId) {
          return Promise.reject(new Error("deviceId ausente para RESTART_PNP_DEVICE."));
        }
        return runConfirmed(
          {
            title: t("confirm.restartDevice.title"),
            description: t("confirm.restartDevice.body"),
            risk: "sensivel",
            snapshot: false,
          },
          () => auth.restartPnpDevice(deviceId),
        );
      }
      if (actionName === "DISABLE_GAME_DVR") {
        return runConfirmed(
          {
            title: t("confirm.gameDvr.title"),
            description: t("confirm.gameDvr.body"),
            risk: "sensivel",
            snapshot: true,
          },
          auth.disableGameDvr,
        );
      }
      if (actionName === "REPAIR_SERVICE") {
        const serviceName = typeof context?.serviceName === "string" ? context.serviceName : null;
        if (!serviceName) {
          return Promise.reject(new Error("serviceName ausente para REPAIR_SERVICE."));
        }
        return runConfirmed(
          {
            title: t("confirm.repairService.title"),
            description: t("confirm.repairService.body"),
            risk: "sensivel",
            snapshot: true,
          },
          () => auth.repairService(serviceName),
        );
      }
      if (actionName === "DISABLE_SERVICE_PERMANENTLY") {
        const serviceName = typeof context?.serviceName === "string" ? context.serviceName : null;
        if (!serviceName) {
          return Promise.reject(new Error("serviceName ausente para DISABLE_SERVICE_PERMANENTLY."));
        }
        return runConfirmed(
          {
            title: t("confirm.disableService.title"),
            description: t("confirm.disableService.body"),
            risk: "sensivel",
            snapshot: true,
          },
          () => auth.disableServicePermanently(serviceName),
        );
      }
      if (actionName === "THROTTLE_DEFENDER_SCANS") {
        return runConfirmed(
          {
            title: t("confirm.defenderThrottle.title"),
            description: t("confirm.defenderThrottle.body"),
            risk: "sensivel",
            snapshot: true,
          },
          auth.throttleDefenderScans,
        );
      }
      if (actionName === "RESTORE_DEFENDER_SCAN_SETTINGS") {
        return runConfirmed(
          {
            title: t("confirm.defenderRestore.title"),
            description: t("confirm.defenderRestore.body"),
            risk: "sensivel",
            snapshot: false,
          },
          auth.restoreDefenderScanSettings,
        );
      }
      if (actionName === "RENEW_DEFENDER_DEFINITIONS") {
        return runConfirmed(
          {
            title: t("confirm.defenderRenew.title"),
            description: t("confirm.defenderRenew.body"),
            risk: "sensivel",
            snapshot: false,
          },
          auth.renewDefenderDefinitions,
        );
      }
      if (actionName === "SCHEDULE_VOLUME_CHECK") {
        const driveLetter = typeof context?.driveLetter === "string" ? context.driveLetter : null;
        if (!driveLetter) {
          return Promise.reject(new Error("driveLetter ausente para SCHEDULE_VOLUME_CHECK."));
        }
        return runConfirmed(
          {
            title: t("confirm.volumeCheck.title", { drive: driveLetter }),
            description: t("confirm.volumeCheck.body"),
            risk: "sensivel",
            snapshot: false,
          },
          () => auth.scheduleVolumeCheck(driveLetter),
        );
      }
      return Promise.reject(new Error(`Ação não suportada localmente: ${actionName}`));
    },
    [runConfirmed, activateGameModeWithUpsell, auth],
  );

  // Ações de Controles e de Meu PC › Saúde, com a mesma confirmação de sempre.
  const controlsActions: ControlsActions = {
    onActivateGameMode: () =>
              runConfirmed(
                {
                  title: t("confirm.gameMode.title"),
                  description: t("confirm.gameMode.body"),
                  risk: "sensivel",
                  snapshot: true,
                },
                activateGameModeWithUpsell,
              ),
    onRestoreOptimizations: () =>
              runConfirmed(
                {
                  title: t("confirm.restoreAll.title"),
                  description: t("confirm.restoreAll.body"),
                  risk: "seguro",
                  snapshot: false,
                },
                auth.restoreOptimizations,
              ),
    onStartSystemFileCheck: () =>
              runConfirmed(
                {
                  title: t("confirm.sfc.title"),
                  description: t("confirm.sfc.body"),
                  risk: "sensivel",
                  snapshot: false,
                },
                auth.startSystemFileCheck,
              ),
    onStartDismRestoreHealth: () =>
              runConfirmed(
                {
                  title: t("confirm.dism.title"),
                  description: t("confirm.dism.body"),
                  risk: "sensivel",
                  snapshot: false,
                },
                auth.startDismRestoreHealth,
              ),
    onDisableStartup: (name, location) =>
              runConfirmed(
                {
                  title: t("confirm.disableStartup.title", { name }),
                  description: t("confirm.disableStartup.body", { name, location: location ?? t("confirm.disableStartup.unknownLocation") }),
                  risk: "sensivel",
                  snapshot: true,
                },
                () => auth.disableStartup(name, location),
              ),
    onRestoreStartup: (name) =>
              runConfirmed(
                {
                  title: name ? t("confirm.restoreStartup.title", { name }) : t("confirm.restoreStartup.titleAll"),
                  description: t("confirm.restoreStartup.body"),
                  risk: "seguro",
                  snapshot: false,
                },
                () => auth.restoreStartup(name),
              ),
    onStopService: (name) =>
              runConfirmed(
                {
                  title: t("confirm.stopService.title", { name }),
                  description: t("confirm.stopService.body"),
                  risk: "sensivel",
                  snapshot: true,
                },
                () => auth.stopService(name),
              ),
    onRestoreService: (name) =>
              runConfirmed(
                {
                  title: name ? t("confirm.restoreService.title", { name }) : t("confirm.restoreService.titleAll"),
                  description: t("confirm.restoreService.body"),
                  risk: "seguro",
                  snapshot: false,
                },
                () => auth.restoreService(name),
              ),
    onSetPowerPlan: (plan) =>
              runConfirmed(
                {
                  title: plan === "high_performance" ? t("confirm.powerPlan.high") : plan === "power_saver" ? t("confirm.powerPlan.saver") : t("confirm.powerPlan.balanced"),
                  description: t("confirm.powerPlan.body"),
                  risk: "sensivel",
                  snapshot: true,
                },
                () => auth.setPowerPlan(plan),
              ),
    onApplyVisualPerformance: () =>
              runConfirmed(
                {
                  title: t("confirm.visualPerformance.title"),
                  description: t("confirm.visualPerformance.body"),
                  risk: "sensivel",
                  snapshot: true,
                },
                auth.applyVisualPerformance,
              ),
    onRestoreVisualPerformance: () =>
              runConfirmed(
                {
                  title: t("confirm.restoreVisual.title"),
                  description: t("confirm.restoreVisual.body"),
                  risk: "seguro",
                  snapshot: false,
                },
                auth.restoreVisualPerformance,
              ),
    onDeepCleanTemp: () =>
              runConfirmed(
                {
                  title: t("confirm.deepTemp.title"),
                  description: t("confirm.deepTemp.body"),
                  risk: "sensivel",
                  snapshot: true,
                },
                auth.cleanTempDeep,
              ),
    onPurgeCleanup: () =>
              runConfirmed(
                {
                  title: t("confirm.purgeQuarantine.title"),
                  description: t("confirm.purgeQuarantine.body"),
                  risk: "sensivel",
                  snapshot: false,
                },
                auth.purgeCleanup,
              ),
    onRestoreGameMode: () =>
              runConfirmed(
                {
                  title: t("confirm.restoreGameMode.title"),
                  description: t("confirm.restoreGameMode.body"),
                  risk: "seguro",
                  snapshot: false,
                },
                auth.restoreGameMode,
              ),
    onApplyPcCleanFast: () =>
              runConfirmed(
                {
                  title: t("confirm.pcCleanFast.title"),
                  description: t("confirm.pcCleanFast.body"),
                  risk: "sensivel",
                  snapshot: true,
                },
                auth.pcCleanFast,
              ),
    onRestorePerformanceSession: (sessionId) =>
              runConfirmed(
                {
                  title: t("confirm.restorePerformanceSession.title"),
                  description: t("confirm.restorePerformanceSession.body"),
                  risk: "seguro",
                  snapshot: false,
                },
                () => auth.restorePerformance(sessionId),
              ),
    onApplyCleanupCategory: (category, mode) =>
              runConfirmed(
                {
                  title: category === "cleanup_quarantine" ? t("confirm.purgeQuarantine.title") : t("confirm.cleanupCategory.title"),
                  description: category === "cleanup_quarantine" ? t("confirm.purgeQuarantine.body") : t("confirm.cleanupCategory.body"),
                  risk: category === "cleanup_quarantine" ? "sensivel" : "seguro",
                  snapshot: category !== "cleanup_quarantine",
                },
                () => auth.applyCleanupCategory(category, mode),
              ),
    onDelayStartupApp: (name, location) =>
              runConfirmed(
                {
                  title: t("confirm.delayStartup.title", { name }),
                  description: t("confirm.delayStartup.body", { name }),
                  risk: "sensivel",
                  snapshot: true,
                },
                () => auth.delayStartup(name, location),
              ),
    onRestoreDelayedStartupApp: (name) =>
              runConfirmed(
                {
                  title: name ? t("confirm.restoreDelayedStartup.title", { name }) : t("confirm.restoreDelayedStartup.titleAll"),
                  description: t("confirm.restoreDelayedStartup.body"),
                  risk: "seguro",
                  snapshot: false,
                },
                () => auth.restoreDelayedStartup(name),
              ),
    onDisableGameDvr: () =>
      runConfirmed(
        {
          title: t("confirm.gameDvr.title"),
          description: t("confirm.gameDvr.body"),
          risk: "sensivel",
          snapshot: true,
        },
        auth.disableGameDvr,
      ),
  };

  if (!auth.ready) {
    return <div className="text-sm text-fg-3">{t("app.loading")}</div>;
  }

  switch (current.screen) {
    case "dashboard":
      return (
        <Suspense fallback={<ViewFallback />}>
          <Dashboard
            user={auth.user}
            status={auth.status}
            telemetry={telemetry}
            busy={auth.busy}
            navigate={navigate}
            onStartAgent={auth.start}
            onCollectSample={auth.collectSample}
            onActivateGameMode={() =>
              runConfirmed(
                {
                  title: t("confirm.gameMode.title"),
                  description: t("confirm.gameMode.body"),
                  risk: "sensivel",
                  snapshot: true,
                },
                activateGameModeWithUpsell,
              )
            }
            onRestoreGameMode={() =>
              runConfirmed(
                {
                  title: t("confirm.gameModeOff.title"),
                  description: t("confirm.gameModeOff.body"),
                  risk: "seguro",
                  snapshot: false,
                },
                auth.restoreGameMode,
              )
            }
            onApplyPcCleanFast={() =>
              runConfirmed(
                {
                  title: t("confirm.pcCleanFast.title"),
                  description: t("confirm.pcCleanFast.body"),
                  risk: "sensivel",
                  snapshot: true,
                },
                auth.pcCleanFast,
              )
            }
            onRestoreAll={() =>
              runConfirmed(
                {
                  title: t("confirm.restoreAll.title"),
                  description: t("confirm.restoreAll.body"),
                  risk: "seguro",
                  snapshot: false,
                },
                async () => {
                  const result = await auth.restoreOptimizations();
                  await Promise.allSettled([auditLog.refresh(), optimizationSnapshots.refresh()]);
                  return result;
                },
              )
            }
            onReleaseMemory={() =>
              runConfirmed(
                {
                  title: t("confirm.releaseMemory.title"),
                  description: t("confirm.releaseMemory.body"),
                  risk: "seguro",
                  snapshot: false,
                },
                async () => {
                  const result = await releaseMemory();
                  await Promise.allSettled([auditLog.refresh()]);
                  return result;
                },
              )
            }
          />
        </Suspense>
      );
    case "history":
      return <History />;
    case "insights":
      return (
        <Suspense fallback={<ViewFallback />}>
          <Insights
            telemetry={telemetry}
            diskNearFullInfo={diskNearFullInfo}
            onOpenDiskUsage={openDiskUsageDetails}
            onOpenNetwork={openNetworkDetails}
            onApplyInsightActionLocally={applyInsightActionLocally}
            onRequestAgentApplyInsight={auth.requestAgentApplyInsight}
            shadowConsentNeeded={shadowConsentNeeded}
            onResolveShadowConsent={resolveShadowConsent}
            onOpenWindowsHealth={() => navigate(route("mypc", "health"))}
          />
        </Suspense>
      );
    case "controls":
      return (
        <Suspense fallback={<ViewFallback />}>
          <Controls
            section={current.section ?? null}
            status={auth.status}
            busy={auth.busy}
            navigate={navigate}
            actions={controlsActions}
          />
        </Suspense>
      );
    case "settings":
      return (
        <Suspense fallback={<ViewFallback />}>
          <Settings
            user={auth.user}
            status={auth.status}
            syncingPlan={auth.syncingPlan}
            onLogin={auth.login}
            onLogout={auth.logout}
            onOpenAccountSettings={auth.openAccountSettings}
            onOpenBilling={auth.openBilling}
            onSyncPlan={auth.syncPlan}
            onOpenHistory={() => navigate(route("history"))}
            onOpenControls={() => navigate(route("controls"))}
          />
        </Suspense>
      );
    case "mypc": {
      const tab = current.tab ?? "performance";
      return (
        <MyPC tab={tab} onTabChange={(next) => navigate(route("mypc", next))}>
          <Suspense fallback={<ViewFallback />}>
            {tab === "performance" ? (
              <Telemetry
                latestSample={auth.sample ?? telemetry}
                agentMode={auth.status?.mode ?? telemetry?.telemetry_mode}
                isReady={Boolean(auth.status?.authenticated && auth.status.registered)}
                busy={auth.busy}
                onCollectSample={auth.collectSample}
                onSetTelemetryMode={auth.setTelemetryMode}
                onOpenDiskUsage={openDiskUsageDetails}
                onOpenNetwork={openNetworkDetails}
              />
            ) : tab === "disk" ? (
              <DiskExplorer
                autoScan={focusDiskUsage}
                onAutoScanHandled={() => setFocusDiskUsage(false)}
                onDiskNearFullDetected={setDiskNearFullInfo}
                onOpenCleanup={() => navigate(route("controls", "cleanup"))}
              />
            ) : tab === "network" ? (
              <Network
                busy={auth.busy}
                isReady={Boolean(auth.status?.authenticated && auth.status.registered)}
                autoTracerouteTarget={focusTracerouteTarget}
                onAutoTracerouteHandled={() => setFocusTracerouteTarget(null)}
                onFlushDnsCache={() =>
                  runConfirmed(
                    {
                      title: t("confirm.flushDns.title"),
                      description: t("confirm.flushDns.body"),
                      risk: "seguro",
                      snapshot: false,
                    },
                    auth.flushDns,
                  )
                }
                onSetDnsServers={(adapterName, dnsServers) =>
                  runConfirmed(
                    {
                      title: t("confirm.setDns.title"),
                      description: t("confirm.setDns.body"),
                      risk: "sensivel",
                      snapshot: true,
                    },
                    () => auth.setDnsServers(adapterName, dnsServers),
                  )
                }
                onResetWinsockCatalog={() =>
                  runConfirmed(
                    {
                      title: t("confirm.resetWinsock.title"),
                      description: t("confirm.resetWinsock.body"),
                      risk: "sensivel",
                      snapshot: false,
                    },
                    auth.resetWinsock,
                  )
                }
                onSetAdapterEnabled={async (adapterName, enabled, context) => {
                  if (context?.riskWarning) {
                    const acknowledged = await requestConfirmation({
                      title: t("confirm.adapterInUse.title"),
                      description: context.riskWarning,
                      risk: "sensivel",
                      snapshot: false,
                    });
                    if (!acknowledged) return false;
                  }
                  const baseDescription = enabled ? t("confirm.adapterEnable.body") : t("confirm.adapterDisable.body");
                  return runConfirmed(
                    {
                      title: enabled ? t("confirm.adapterEnable.title") : t("confirm.adapterDisable.title"),
                      description: context?.reason ? `${context.reason}\n\n${baseDescription}` : baseDescription,
                      risk: "sensivel",
                      snapshot: true,
                    },
                    () => auth.setAdapterEnabled(adapterName, enabled),
                  );
                }}
                onApplyNetworkTune={(request) =>
                  runConfirmed(
                    {
                      title: t("confirm.networkTune.title"),
                      description: t("confirm.networkTune.body"),
                      risk: "sensivel",
                      snapshot: true,
                    },
                    () => auth.applyNetworkTune(request),
                  )
                }
                onRestartWindows={() =>
                  runConfirmed(
                    {
                      title: t("confirm.restartForNetwork.title"),
                      description: t("confirm.restartForNetwork.body"),
                      risk: "sensivel",
                      snapshot: false,
                    },
                    auth.restartWindowsNow,
                  )
                }
              />
            ) : (
              <WindowsHealth
                telemetry={telemetry}
                shadowConsentNeeded={shadowConsentNeeded}
                onResolveShadowConsent={resolveShadowConsent}
                onRestartWindows={() =>
                  runConfirmed(
                    {
                      title: t("confirm.restartWindows.title"),
                      description: t("confirm.restartWindows.body"),
                      risk: "sensivel",
                      snapshot: false,
                    },
                    auth.restartWindowsNow,
                  )
                }
                onRestartDevice={(deviceId) => applyInsightActionLocally("RESTART_PNP_DEVICE", { deviceId })}
                onStartSfc={controlsActions.onStartSystemFileCheck}
                onStartDism={controlsActions.onStartDismRestoreHealth}
                onOpenInsights={() => handleViewChange("insights")}
              />
            )}
          </Suspense>
        </MyPC>
      );
    }
  }
}
