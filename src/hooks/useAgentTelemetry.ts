import { useEffect, useState } from "react";
import {
  getAgentTelemetrySnapshot,
  listenToAgentSessionInvalidated,
  listenToAgentTelemetry,
  type AgentTelemetrySnapshot,
} from "@/services/tauri/agent";

export function useAgentTelemetry() {
  const [snapshot, setSnapshot] = useState<AgentTelemetrySnapshot | null>(null);

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
      if (active) setSnapshot(nextSnapshot);
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
