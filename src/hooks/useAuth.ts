import { useCallback, useEffect, useMemo, useState } from "react";
import {
  activateAgentGameMode,
  activateFocusMode,
  applyCleanupCategory as applyAgentCleanupCategory,
  applyPcCleanFastProfile,
  applyVisualPerformanceMode,
  collectAgentTelemetrySample,
  completeAuthFromDeepLink,
  deepCleanTemp,
  delayStartupApp,
  disableStartupApp,
  flushDnsCache,
  getAgentStatus,
  isTauriRuntime,
  logoutAgent,
  openAgentAccountSettings,
  openAgentBilling,
  openAgentLogin,
  registerDeepLinkHandlers,
  restoreActiveGameMode,
  restoreDelayedStartupApp,
  restoreFocusSession,
  restorePendingOptimizations,
  restoreStartupApp,
  restoreVisualEffects,
  restoreWindowsService,
  purgeCleanupQuarantine,
  resetWinsockCatalog,
  setAgentTelemetryMode,
  setDnsServers as setDnsServersAction,
  setPowerPlanBalanced,
  setPowerPlanHighPerformance,
  setPowerPlanPowerSaver,
  startAgent,
  stopWindowsService,
  listenToAgentSessionInvalidated,
  listenToPlanSynced,
  restorePerformanceSession,
  runPerformanceScan,
  syncAccountPlan,
  type AgentStatus,
  type AgentTelemetrySample,
  type FocusModeProfile,
} from "@/services/tauri/agent";
import { captureTelemetry } from "@/services/telemetry";

export type User = {
  name: string;
  email?: string | null;
  plan: string;
  hasPaidPlan: boolean;
  sessionId: string;
};
export type AgentMessage = { key: string; params?: Record<string, string | number | boolean> };

export function canUseAutomaticGameMode(status?: AgentStatus | null) {
  if (!status?.authenticated || !status.registered) return false;
  const normalizedPlan = (status.plan || "")
    .trim()
    .toLowerCase()
    .replace(/[-\s]+/g, "_");
  return Boolean(
    status.has_paid_plan &&
      ["pro", "family", "family_friends", "familyfriends"].includes(normalizedPlan),
  );
}

export function canUsePaidGameMode(status?: AgentStatus | null) {
  return Boolean(status?.authenticated && status.registered && status.has_paid_plan);
}

export function useAuth() {
  const [status, setStatus] = useState<AgentStatus | null>(null);
  const [sample, setSample] = useState<AgentTelemetrySample | null>(null);
  const [message, setMessage] = useState<AgentMessage>({ key: "agent.messages.initializing" });
  const [ready, setReady] = useState(false);
  const [busy, setBusy] = useState(false);
  const [syncingPlan, setSyncingPlan] = useState(false);

  const refreshStatus = useCallback(async () => {
    const nextStatus = await getAgentStatus();
    setStatus(nextStatus);
    return nextStatus;
  }, []);

  const syncPlan = useCallback(async () => {
    setSyncingPlan(true);
    try {
      const nextStatus = await syncAccountPlan();
      setStatus(nextStatus);
      setMessage({
        key: nextStatus.plan_sync_error ? "agent.messages.planSyncFailed" : "agent.messages.planSynced",
      });
      captureTelemetry({
        name: nextStatus.plan_sync_error ? "plan_sync_failed" : "plan_synced",
        category: "agent",
      });
      return nextStatus;
    } catch (error) {
      setMessage({ key: "agent.messages.error", params: { message: String(error) } });
      captureTelemetry({ name: "plan_sync_failed", category: "agent" });
      throw error;
    } finally {
      setSyncingPlan(false);
    }
  }, []);

  const handleDeepLink = useCallback(async (url: string) => {
    setBusy(true);
    try {
      const nextStatus = await completeAuthFromDeepLink(url);
      setStatus(nextStatus);
      setMessage({
        key: nextStatus.registered ? "agent.messages.deepLinkSuccess" : "agent.messages.hardwareSecretMissing",
      });
      captureTelemetry({ name: "auth_deep_link_completed", category: "agent" });
    } catch (error) {
      setMessage({ key: "agent.messages.error", params: { message: String(error) } });
      captureTelemetry({ name: "auth_deep_link_failed", category: "agent" });
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    let disposeDeepLinks: (() => void) | undefined;
    refreshStatus()
      .then(() => setMessage({ key: "agent.messages.ready" }))
      .catch((error) => setMessage({ key: "agent.messages.error", params: { message: String(error) } }))
      .finally(() => setReady(true));

    registerDeepLinkHandlers((url) => void handleDeepLink(url)).then((dispose) => {
      disposeDeepLinks = dispose;
    });

    let disposeInvalidated: (() => void) | undefined;
    listenToAgentSessionInvalidated(() => {
      setSample(null);
      setMessage({ key: "agent.messages.deviceInvalidated" });
      void refreshStatus().catch(() => undefined);
    }).then((dispose) => {
      disposeInvalidated = dispose;
    });

    let disposePlanSynced: (() => void) | undefined;
    listenToPlanSynced((nextStatus) => {
      setStatus(nextStatus);
    }).then((dispose) => {
      disposePlanSynced = dispose;
    });

    return () => {
      disposeDeepLinks?.();
      disposeInvalidated?.();
      disposePlanSynced?.();
    };
  }, [handleDeepLink, refreshStatus]);

  const runAction = useCallback(async <T,>(action: () => Promise<T>, options?: { rethrow?: boolean }): Promise<T | undefined> => {
    setBusy(true);
    try {
      return await action();
    } catch (error) {
      setMessage({ key: "agent.messages.error", params: { message: String(error) } });
      if (options?.rethrow) throw error;
      return undefined;
    } finally {
      setBusy(false);
    }
  }, []);

  const login = useCallback(async () => {
    if (!isTauriRuntime()) {
      const url = await openAgentLogin();
      window.open(url, "_blank", "noopener,noreferrer");
      setMessage({ key: "agent.messages.browserLoginUnavailable", params: { url } });
      captureTelemetry({ name: "browser_login_opened", category: "agent" });
      return;
    }

    await runAction(async () => {
      const url = await openAgentLogin();
      setMessage({ key: "agent.messages.loginOpened", params: { url } });
      captureTelemetry({ name: "agent_login_opened", category: "agent" });
    });
  }, [runAction]);

  const logout = useCallback(async () => {
    if (!isTauriRuntime()) {
      setMessage({ key: "agent.messages.browserLogoutUnavailable" });
      captureTelemetry({ name: "browser_preview_logout", category: "agent" });
      return;
    }

    await runAction(async () => {
      const nextStatus = await logoutAgent();
      setStatus(nextStatus);
      setMessage({ key: "agent.messages.logout" });
      captureTelemetry({ name: "agent_logout", category: "agent" });
    });
  }, [runAction]);

  const openAccountSettings = useCallback(async () => {
    await runAction(async () => {
      const url = await openAgentAccountSettings();
      if (!isTauriRuntime()) {
        window.open(url, "_blank", "noopener,noreferrer");
      }
      setMessage({ key: "agent.messages.accountSettingsOpened", params: { url } });
      captureTelemetry({ name: "agent_account_settings_opened", category: "agent" });
    });
  }, [runAction]);

  const openBilling = useCallback(async () => {
    await runAction(async () => {
      const url = await openAgentBilling();
      if (!isTauriRuntime()) {
        window.open(url, "_blank", "noopener,noreferrer");
      }
      setMessage({ key: "agent.messages.billingOpened", params: { url } });
      captureTelemetry({ name: "agent_billing_opened", category: "agent" });
    });
  }, [runAction]);

  const start = useCallback(async () => {
    await runAction(async () => {
      const nextStatus = await startAgent();
      setStatus(nextStatus);
      setMessage({ key: "agent.messages.agentStarted" });
      captureTelemetry({ name: "agent_started", category: "agent" });
    });
  }, [runAction]);

  const activateGameMode = useCallback(async () => {
    await runAction(async () => {
      if (!canUsePaidGameMode(status)) {
        throw new Error("Modo Gamer esta disponivel apenas para planos pagos.");
      }
      const result = await activateAgentGameMode();
      setStatus(result.status);
      setMessage({
        key: result.success ? "agent.messages.gameModeApplied" : "agent.messages.gameModeFailed",
        params: { message: result.message },
      });
      captureTelemetry({
        name: result.success ? "game_mode_applied" : "game_mode_failed",
        category: "agent",
      });
    });
  }, [runAction, status]);

  const activateGamePerformanceMode = useCallback(async () => {
    const result = await runAction(async () => {
      if (!canUsePaidGameMode(status)) {
        throw new Error("Modo Gamer esta disponivel apenas para planos pagos.");
      }
      const result = await applyPcCleanFastProfile({
        includeStartup: false,
        includeCleanup: true,
        includeBackground: true,
        includeNetwork: true,
        includeGaming: true,
      });
      setMessage({
        key: result.success ? "agent.messages.gameModeApplied" : "agent.messages.gameModeFailed",
        params: { message: result.message },
      });
      captureTelemetry({
        name: result.success ? "game_performance_mode_applied" : "game_performance_mode_failed",
        category: "agent",
      });
      return result;
    }, { rethrow: true });
    if (result && !result.success) throw new Error(result.message);
    return result;
  }, [runAction, status]);

  const restoreOptimizations = useCallback(async () => {
    const result = await runAction(async () => {
      const result = await restorePendingOptimizations();
      setMessage({
        key: "agent.messages.restoreApplied",
        params: {
          restored: result.restored_snapshots,
          failed: result.failed_snapshots,
          entries: result.restored_entries,
        },
      });
      captureTelemetry({
        name: "optimization_restore_requested",
        category: "agent",
        properties: {
          restored_snapshots: result.restored_snapshots,
          failed_snapshots: result.failed_snapshots,
          restored_entries: result.restored_entries,
        },
      });
      return result;
    }, { rethrow: true });
    if (result && (result.failed_snapshots > 0 || result.failed_entries > 0)) {
      throw new Error(result.messages.join(" ") || "Falha ao restaurar snapshots.");
    }
    return result;
  }, [runAction]);

  const restoreGameMode = useCallback(async () => {
    const result = await runAction(async () => {
      const result = await restoreActiveGameMode();
      setMessage({
        key: "agent.messages.restoreApplied",
        params: {
          restored: result.restored_snapshots,
          failed: result.failed_snapshots,
          entries: result.restored_entries,
        },
      });
      captureTelemetry({
        name: "game_mode_restore_requested",
        category: "agent",
        properties: {
          restored_snapshots: result.restored_snapshots,
          failed_snapshots: result.failed_snapshots,
          restored_entries: result.restored_entries,
        },
      });
      return result;
    }, { rethrow: true });
    if (result && (result.failed_snapshots > 0 || result.failed_entries > 0)) {
      throw new Error(result.messages.join(" ") || "Falha ao restaurar Modo Gamer.");
    }
    return result;
  }, [runAction]);

  const activateFocus = useCallback(async (profile: FocusModeProfile = "focus") => {
    const result = await runAction(async () => {
      const result = await activateFocusMode(profile);
      const nextStatus = await getAgentStatus();
      setStatus(nextStatus);
      setMessage({
        key: result.success ? "agent.messages.optimizationActionApplied" : "agent.messages.optimizationActionFailed",
        params: { message: result.message },
      });
      captureTelemetry({
        name: result.success ? "focus_mode_applied" : "focus_mode_failed",
        category: "agent",
        properties: { profile },
      });
      return result;
    }, { rethrow: true });
    if (result && !result.success) throw new Error(result.message);
    return result;
  }, [runAction]);

  const restoreFocus = useCallback(async () => {
    const result = await runAction(async () => {
      const result = await restoreFocusSession();
      const nextStatus = await getAgentStatus();
      setStatus(nextStatus);
      setMessage({
        key: "agent.messages.restoreApplied",
        params: {
          restored: result.restored_snapshots,
          failed: result.failed_snapshots,
          entries: result.restored_entries,
        },
      });
      captureTelemetry({
        name: "focus_mode_restore_requested",
        category: "agent",
        properties: {
          restored_snapshots: result.restored_snapshots,
          failed_snapshots: result.failed_snapshots,
          restored_entries: result.restored_entries,
        },
      });
      return result;
    }, { rethrow: true });
    if (result && (result.failed_snapshots > 0 || result.failed_entries > 0)) {
      throw new Error(result.messages.join(" ") || "Falha ao restaurar Modo Foco.");
    }
    return result;
  }, [runAction]);

  const disableStartup = useCallback(
    async (name: string, location?: string | null) => {
      const result = await runAction(async () => {
        const result = await disableStartupApp(name, location);
        setMessage({
          key: result.success ? "agent.messages.optimizationActionApplied" : "agent.messages.optimizationActionFailed",
          params: { message: result.message },
        });
        captureTelemetry({
          name: result.success ? "startup_app_disabled" : "startup_app_disable_failed",
          category: "agent",
          properties: { target: name },
        });
        return result;
      }, { rethrow: true });
      if (result && !result.success) throw new Error(result.message);
      return result;
    },
    [runAction],
  );

  const restoreStartup = useCallback(
    async (name?: string | null) => {
      const result = await runAction(async () => {
        const result = await restoreStartupApp(name);
        setMessage({
          key: result.success ? "agent.messages.optimizationActionApplied" : "agent.messages.optimizationActionFailed",
          params: { message: result.message },
        });
        captureTelemetry({
          name: result.success ? "startup_app_restored" : "startup_app_restore_failed",
          category: "agent",
          properties: { target: name ?? "all" },
        });
        return result;
      }, { rethrow: true });
      if (result && !result.success) throw new Error(result.message);
      return result;
    },
    [runAction],
  );

  const stopService = useCallback(
    async (name: string) => {
      const result = await runAction(async () => {
        const result = await stopWindowsService(name);
        setMessage({
          key: result.success ? "agent.messages.optimizationActionApplied" : "agent.messages.optimizationActionFailed",
          params: { message: result.message },
        });
        captureTelemetry({
          name: result.success ? "service_stopped" : "service_stop_failed",
          category: "agent",
          properties: { target: name },
        });
        return result;
      }, { rethrow: true });
      if (result && !result.success) throw new Error(result.message);
      return result;
    },
    [runAction],
  );

  const restoreService = useCallback(
    async (name?: string | null) => {
      const result = await runAction(async () => {
        const result = await restoreWindowsService(name);
        setMessage({
          key: result.success ? "agent.messages.optimizationActionApplied" : "agent.messages.optimizationActionFailed",
          params: { message: result.message },
        });
        captureTelemetry({
          name: result.success ? "service_restored" : "service_restore_failed",
          category: "agent",
          properties: { target: name ?? "all" },
        });
        return result;
      }, { rethrow: true });
      if (result && !result.success) throw new Error(result.message);
      return result;
    },
    [runAction],
  );

  const setPowerPlan = useCallback(
    async (plan: "high_performance" | "balanced" | "power_saver") => {
      const result = await runAction(async () => {
        const result =
          plan === "high_performance"
            ? await setPowerPlanHighPerformance()
            : plan === "power_saver"
              ? await setPowerPlanPowerSaver()
              : await setPowerPlanBalanced();
        setMessage({
          key: result.success ? "agent.messages.optimizationActionApplied" : "agent.messages.optimizationActionFailed",
          params: { message: result.message },
        });
        captureTelemetry({
          name: result.success ? "power_plan_changed" : "power_plan_change_failed",
          category: "agent",
          properties: { plan },
        });
        return result;
      }, { rethrow: true });
      if (result && !result.success) throw new Error(result.message);
      return result;
    },
    [runAction],
  );

  const flushDns = useCallback(async () => {
    const result = await runAction(async () => {
      const result = await flushDnsCache();
      setMessage({
        key: result.success ? "agent.messages.optimizationActionApplied" : "agent.messages.optimizationActionFailed",
        params: { message: result.message },
      });
      captureTelemetry({
        name: result.success ? "dns_cache_flushed" : "dns_cache_flush_failed",
        category: "agent",
        properties: {},
      });
      return result;
    }, { rethrow: true });
    if (result && !result.success) throw new Error(result.message);
    return result;
  }, [runAction]);

  const setDnsServers = useCallback(
    async (adapterName: string, dnsServers: string[]) => {
      const result = await runAction(async () => {
        const result = await setDnsServersAction(adapterName, dnsServers);
        setMessage({
          key: result.success ? "agent.messages.optimizationActionApplied" : "agent.messages.optimizationActionFailed",
          params: { message: result.message },
        });
        captureTelemetry({
          name: result.success ? "dns_servers_changed" : "dns_servers_change_failed",
          category: "agent",
          properties: { adapter: adapterName },
        });
        return result;
      }, { rethrow: true });
      if (result && !result.success) throw new Error(result.message);
      return result;
    },
    [runAction],
  );

  const resetWinsock = useCallback(async () => {
    const result = await runAction(async () => {
      const result = await resetWinsockCatalog();
      setMessage({
        key: result.success ? "agent.messages.optimizationActionApplied" : "agent.messages.optimizationActionFailed",
        params: { message: result.message },
      });
      captureTelemetry({
        name: result.success ? "winsock_catalog_reset" : "winsock_catalog_reset_failed",
        category: "agent",
        properties: {},
      });
      return result;
    }, { rethrow: true });
    if (result && !result.success) throw new Error(result.message);
    return result;
  }, [runAction]);

  const applyVisualPerformance = useCallback(async () => {
    const result = await runAction(async () => {
      const result = await applyVisualPerformanceMode();
      setMessage({
        key: result.success ? "agent.messages.optimizationActionApplied" : "agent.messages.optimizationActionFailed",
        params: { message: result.message },
      });
      captureTelemetry({
        name: result.success ? "visual_performance_applied" : "visual_performance_failed",
        category: "agent",
      });
      return result;
    }, { rethrow: true });
    if (result && !result.success) throw new Error(result.message);
    return result;
  }, [runAction]);

  const restoreVisualPerformance = useCallback(async () => {
    const result = await runAction(async () => {
      const result = await restoreVisualEffects();
      setMessage({
        key: result.success ? "agent.messages.optimizationActionApplied" : "agent.messages.optimizationActionFailed",
        params: { message: result.message },
      });
      captureTelemetry({
        name: result.success ? "visual_performance_restored" : "visual_performance_restore_failed",
        category: "agent",
      });
      return result;
    }, { rethrow: true });
    if (result && !result.success) throw new Error(result.message);
    return result;
  }, [runAction]);

  const cleanTempDeep = useCallback(async () => {
    const result = await runAction(async () => {
      const result = await deepCleanTemp();
      setMessage({
        key: result.success ? "agent.messages.optimizationActionApplied" : "agent.messages.optimizationActionFailed",
        params: { message: result.message },
      });
      captureTelemetry({
        name: result.success ? "temp_deep_clean_applied" : "temp_deep_clean_failed",
        category: "agent",
      });
      return result;
    }, { rethrow: true });
    if (result && !result.success) throw new Error(result.message);
    return result;
  }, [runAction]);

  const purgeCleanup = useCallback(async () => {
    const result = await runAction(async () => {
      const result = await purgeCleanupQuarantine();
      setMessage({
        key: result.success ? "agent.messages.optimizationActionApplied" : "agent.messages.optimizationActionFailed",
        params: { message: result.message },
      });
      captureTelemetry({
        name: result.success ? "cleanup_quarantine_purged" : "cleanup_quarantine_purge_failed",
        category: "agent",
      });
      return result;
    }, { rethrow: true });
    if (result && !result.success) throw new Error(result.message);
    return result;
  }, [runAction]);

  const applyCleanupCategory = useCallback(async (category: string, mode?: string | null) => {
    const result = await runAction(async () => {
      const result = await applyAgentCleanupCategory(category, mode ?? "safe");
      setMessage({
        key: result.success ? "agent.messages.optimizationActionApplied" : "agent.messages.optimizationActionFailed",
        params: { message: result.message },
      });
      captureTelemetry({
        name: result.success ? "cleanup_category_applied" : "cleanup_category_failed",
        category: "agent",
        properties: { category },
      });
      return result;
    }, { rethrow: true });
    if (result && !result.success) throw new Error(result.message);
    return result;
  }, [runAction]);

  const delayStartup = useCallback(async (name: string, location?: string | null) => {
    const result = await runAction(async () => {
      const result = await delayStartupApp(name, location, 120);
      setMessage({
        key: result.success ? "agent.messages.optimizationActionApplied" : "agent.messages.optimizationActionFailed",
        params: { message: result.message },
      });
      captureTelemetry({
        name: result.success ? "startup_app_delayed" : "startup_app_delay_failed",
        category: "agent",
        properties: { target: name },
      });
      return result;
    }, { rethrow: true });
    if (result && !result.success) throw new Error(result.message);
    return result;
  }, [runAction]);

  const restoreDelayedStartup = useCallback(async (name?: string | null) => {
    const result = await runAction(async () => {
      const result = await restoreDelayedStartupApp(name);
      setMessage({
        key: result.success ? "agent.messages.optimizationActionApplied" : "agent.messages.optimizationActionFailed",
        params: { message: result.message },
      });
      captureTelemetry({
        name: result.success ? "startup_app_delay_restored" : "startup_app_delay_restore_failed",
        category: "agent",
        properties: { target: name ?? "all" },
      });
      return result;
    }, { rethrow: true });
    if (result && !result.success) throw new Error(result.message);
    return result;
  }, [runAction]);

  const performanceScan = useCallback(
    async (mode: "baseline" | "after" | "quick" = "quick") => {
      return runAction(async () => {
        const report = await runPerformanceScan(mode);
        setMessage({
          key: "agent.messages.optimizationActionApplied",
          params: { message: `Performance score ${report.overallScore}` },
        });
        captureTelemetry({
          name: "performance_scan_completed",
          category: "agent",
          properties: {
            mode,
            overall_score: report.overallScore,
            measured_gain_percent: report.measuredGainPercent ?? 0,
          },
        });
        return report;
      }, { rethrow: true });
    },
    [runAction],
  );

  const pcCleanFast = useCallback(async () => {
    const result = await runAction(async () => {
      const result = await applyPcCleanFastProfile({
        includeStartup: true,
        includeCleanup: true,
        includeBackground: true,
        includeNetwork: true,
        includeGaming: true,
      });
      setMessage({
        key: result.success ? "agent.messages.optimizationActionApplied" : "agent.messages.optimizationActionFailed",
        params: { message: result.message },
      });
      captureTelemetry({
        name: result.success ? "pc_clean_fast_applied" : "pc_clean_fast_failed",
        category: "agent",
      });
      return result;
    }, { rethrow: true });
    if (result && !result.success) throw new Error(result.message);
    return result;
  }, [runAction]);

  const restorePerformance = useCallback(async (sessionId?: string | null) => {
    const result = await runAction(async () => {
      const result = await restorePerformanceSession(sessionId);
      setMessage({
        key: result.success ? "agent.messages.optimizationActionApplied" : "agent.messages.optimizationActionFailed",
        params: { message: result.message },
      });
      captureTelemetry({
        name: result.success ? "performance_session_restored" : "performance_session_restore_failed",
        category: "agent",
      });
      return result;
    }, { rethrow: true });
    if (result && !result.success) throw new Error(result.message);
    return result;
  }, [runAction]);

  const setTelemetryMode = useCallback(
    async (mode: "normal" | "realtime") => {
      await runAction(async () => {
        const nextStatus = await setAgentTelemetryMode(mode);
        setStatus(nextStatus);
        setMessage({ key: mode === "realtime" ? "agent.messages.realtime" : "agent.messages.normal" });
        captureTelemetry({ name: "agent_mode_changed", category: "agent", properties: { mode } });
      });
    },
    [runAction],
  );

  const collectSample = useCallback(async () => {
    setBusy(true);
    try {
      const nextSample = await collectAgentTelemetrySample();
      setSample(nextSample);
      setMessage({ key: "agent.messages.sample" });
      captureTelemetry({ name: "agent_sample_collected", category: "agent" });
      return nextSample;
    } catch (error) {
      setMessage({ key: "agent.messages.error", params: { message: String(error) } });
      captureTelemetry({ name: "agent_sample_failed", category: "agent" });
      throw error;
    } finally {
      setBusy(false);
    }
  }, []);

  const user = useMemo<User | null>(() => {
    if (!status?.authenticated) return null;

    return {
      name: status.user_name?.trim() || "Conta conectada",
      email: status.user_email,
      plan: status.plan || "starter",
      hasPaidPlan: status.has_paid_plan,
      sessionId: status.hw_id ?? "desktop-session",
    };
  }, [status]);

  return {
    user,
    ready,
    status,
    sample,
    message,
    busy,
    syncingPlan,
    syncPlan,
    login,
    logout,
    openAccountSettings,
    openBilling,
    refreshStatus,
    start,
    activateGameMode,
    activateGamePerformanceMode,
    activateFocus,
    restoreOptimizations,
    restoreGameMode,
    restoreFocus,
    disableStartup,
    restoreStartup,
    stopService,
    restoreService,
    setPowerPlan,
    flushDns,
    setDnsServers,
    resetWinsock,
    applyVisualPerformance,
    restoreVisualPerformance,
    cleanTempDeep,
    purgeCleanup,
    applyCleanupCategory,
    delayStartup,
    restoreDelayedStartup,
    performanceScan,
    pcCleanFast,
    restorePerformance,
    setTelemetryMode,
    collectSample,
  };
}
