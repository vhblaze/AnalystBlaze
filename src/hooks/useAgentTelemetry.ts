import { useEffect, useRef, useState, type MutableRefObject } from "react";
import {
  getAgentTelemetrySnapshot,
  listenToAgentSessionInvalidated,
  listenToAgentTelemetry,
  type AgentTelemetrySnapshot,
} from "@/services/tauri/agent";

export function useAgentTelemetry() {
  const [snapshot, setSnapshot] = useState<AgentTelemetrySnapshot | null>(null);
  const lastCriticalNotificationAt = useRef(0);

  useEffect(() => {
    let active = true;
    let disposeTelemetry: (() => void) | undefined;
    let disposeInvalidated: (() => void) | undefined;

    getAgentTelemetrySnapshot()
      .then((initialSnapshot) => {
        if (active && initialSnapshot) setSnapshot(initialSnapshot);
      })
      .catch(() => undefined);

    listenToAgentTelemetry((nextSnapshot) => {
      if (active) {
        setSnapshot(nextSnapshot);
        maybeNotifyCriticalState(nextSnapshot, lastCriticalNotificationAt);
      }
    }).then((dispose) => {
      disposeTelemetry = dispose;
    });

    listenToAgentSessionInvalidated(() => {
      if (active) setSnapshot(null);
    }).then((dispose) => {
      disposeInvalidated = dispose;
    });

    return () => {
      active = false;
      disposeTelemetry?.();
      disposeInvalidated?.();
    };
  }, []);

  return snapshot;
}

function maybeNotifyCriticalState(
  snapshot: AgentTelemetrySnapshot,
  lastCriticalNotificationAt: MutableRefObject<number>,
) {
  if (snapshot.health_level !== "critical") return;
  if (typeof Notification === "undefined") return;
  if (Date.now() - lastCriticalNotificationAt.current < 10 * 60 * 1000) return;

  const notify = () => {
    lastCriticalNotificationAt.current = Date.now();
    new Notification("AnalystBlaze", {
      body: "Estado critico detectado no PC. Abra o dashboard para revisar.",
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
