import { useCallback } from "react";
import { captureTelemetry, type TelemetryEventInput } from "@/services/telemetry";

export function useTelemetry(category = "ui") {
  return useCallback(
    (name: string, properties?: TelemetryEventInput["properties"]) => {
      captureTelemetry({ name, category, properties });
    },
    [category],
  );
}
