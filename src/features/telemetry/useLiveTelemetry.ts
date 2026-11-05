import {
  getAgentTelemetrySnapshot,
  listenToAgentSessionInvalidated,
  listenToAgentTelemetry,
  type AgentTelemetrySnapshot,
} from "@/services/tauri/agent";
import { createAgentResource } from "@/features/shared/agentResource";

let lastCriticalNotificationAt = 0;

function maybeNotifyCriticalState(snapshot: AgentTelemetrySnapshot) {
  if (snapshot.health_level !== "critical") return;
  if (typeof Notification === "undefined") return;
  if (Date.now() - lastCriticalNotificationAt < 10 * 60 * 1000) return;

  const notify = () => {
    lastCriticalNotificationAt = Date.now();
    new Notification("AnalystBlaze", {
      body: "Estado crítico detectado no PC. Abra o dashboard para revisar.",
      silent: false,
    });
  };

  if (Notification.permission === "granted") {
    notify();
  } else if (Notification.permission === "default") {
    void Notification.requestPermission().then((permission) => {
      if (permission === "granted") notify();
    });
  }
}

/**
 * Última amostra de telemetria do agente (CPU, GPU, memória, disco, saúde).
 * Um único ouvinte para o app inteiro: Dashboard, Meu PC › Desempenho e
 * Insights mostram o mesmo número.
 */
export const liveTelemetry = createAgentResource<AgentTelemetrySnapshot | null>({
  name: "telemetry.live",
  fetch: getAgentTelemetrySnapshot,
  staleMs: 5_000,
  subscribe: async (push) => {
    const disposeTelemetry = await listenToAgentTelemetry((snapshot) => {
      push(snapshot);
      maybeNotifyCriticalState(snapshot);
    });
    const disposeInvalidated = await listenToAgentSessionInvalidated(() => push(null));
    return () => {
      disposeTelemetry();
      disposeInvalidated();
    };
  },
});

export function useLiveTelemetry(): AgentTelemetrySnapshot | null {
  return liveTelemetry.use().data ?? null;
}
