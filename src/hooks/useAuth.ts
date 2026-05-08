import { useCallback, useEffect, useMemo, useState } from "react";
import {
  collectAgentTelemetrySample,
  completeAuthFromDeepLink,
  getAgentStatus,
  isTauriRuntime,
  logoutAgent,
  openAgentLogin,
  registerDeepLinkHandlers,
  setAgentTelemetryMode,
  startAgent,
  type AgentStatus,
  type AgentTelemetrySample,
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

export function useAuth() {
  const [status, setStatus] = useState<AgentStatus | null>(null);
  const [sample, setSample] = useState<AgentTelemetrySample | null>(null);
  const [message, setMessage] = useState<AgentMessage>({ key: "agent.messages.initializing" });
  const [ready, setReady] = useState(false);
  const [busy, setBusy] = useState(false);

  const refreshStatus = useCallback(async () => {
    const nextStatus = await getAgentStatus();
    setStatus(nextStatus);
    return nextStatus;
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

    return () => {
      disposeDeepLinks?.();
    };
  }, [handleDeepLink, refreshStatus]);

  const runAction = useCallback(async (action: () => Promise<void>) => {
    setBusy(true);
    try {
      await action();
    } catch (error) {
      setMessage({ key: "agent.messages.error", params: { message: String(error) } });
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

  const start = useCallback(async () => {
    await runAction(async () => {
      const nextStatus = await startAgent();
      setStatus(nextStatus);
      setMessage({ key: "agent.messages.agentStarted" });
      captureTelemetry({ name: "agent_started", category: "agent" });
    });
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
    login,
    logout,
    refreshStatus,
    start,
    setTelemetryMode,
    collectSample,
  };
}
