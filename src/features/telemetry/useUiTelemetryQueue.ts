import { useEffect, useState } from "react";
import { getTelemetryQueueSize } from "@/services/telemetry";

const POLL_MS = 10_000;

/** Quantos eventos de uso da interface esperam envio. Uma leitura, um timer. */
export function useUiTelemetryQueue() {
  const [size, setSize] = useState(getTelemetryQueueSize);
  useEffect(() => {
    const id = window.setInterval(() => setSize(getTelemetryQueueSize()), POLL_MS);
    return () => window.clearInterval(id);
  }, []);
  return { size, refresh: () => setSize(getTelemetryQueueSize()) };
}
