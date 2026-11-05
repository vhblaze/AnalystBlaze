import { useLiveTelemetry } from "@/features/telemetry/useLiveTelemetry";

/** Mantido para quem ainda importa daqui; a fonte é features/telemetry. */
export function useAgentTelemetry() {
  return useLiveTelemetry();
}
