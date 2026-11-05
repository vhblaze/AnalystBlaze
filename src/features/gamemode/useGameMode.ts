import {
  getActiveGameModeSession,
  getFrameMeasurements,
  type FrameMeasurementEntry,
  getWeeklyGameModeUsage,
  listenToGameModeUsage,
  type GameModeSession,
  type GameModeUsage,
} from "@/services/tauri/agent";
import { createAgentResource } from "@/features/shared/agentResource";

/** Minutos do Modo Jogo usados nesta semana (Starter). Atualiza por evento do agente. */
export const weeklyGameModeUsage = createAgentResource<GameModeUsage | null>({
  name: "gamemode.weeklyUsage",
  fetch: getWeeklyGameModeUsage,
  subscribe: (push) => listenToGameModeUsage(push),
});

/**
 * L4: antes/depois em FPS do Modo Jogo automático (20 s antes, 20 s
 * depois, mesmo jogo em primeiro plano). Guardado só no PC.
 */
export const frameMeasurements = createAgentResource<FrameMeasurementEntry[]>({
  name: "gamemode.frameMeasurements",
  fetch: () => getFrameMeasurements(10),
  staleMs: 60_000,
});

/** A medição mais nova que deu número, e a mais nova de todas (pode ter sido pulada). */
export function latestFrameMeasurement(entries: FrameMeasurementEntry[] | undefined) {
  const list = entries ?? [];
  const valid = list.find((entry) => entry.measurement.verdict && entry.measurement.verdict !== "incomplete") ?? null;
  const newest = list[0] ?? null;
  const skippedAfter = newest && newest !== valid && newest.measurement.skipped ? newest : null;
  return { valid, skippedAfter };
}

/** Motivo de uma medição pulada, como chave de tradução. */
export function skippedReasonKey(skipped: string | undefined): "notInFront" | "tooFewFrames" | "capture" {
  if (!skipped) return "capture";
  if (skipped.endsWith("game_not_in_foreground")) return "notInFront";
  if (skipped.endsWith("too_few_frames")) return "tooFewFrames";
  return "capture";
}

/** Sessão do Modo Jogo ativa agora, ou null. */
export const activeGameModeSession = createAgentResource<GameModeSession | null>({
  name: "gamemode.activeSession",
  fetch: getActiveGameModeSession,
  staleMs: 5_000,
});

/**
 * Modo Jogo: uso semanal + sessão ativa, a mesma fonte no Dashboard e em
 * Controles. `trackUsage=false` (plano pago, sem limite) não lê o uso semanal.
 */
export function useGameMode({ trackUsage = true }: { trackUsage?: boolean } = {}) {
  const usage = weeklyGameModeUsage.use({ auto: trackUsage });
  const session = activeGameModeSession.use();
  return {
    usage: trackUsage ? (usage.data ?? null) : null,
    refreshUsage: usage.refresh,
    session: session.data ?? null,
    sessionLoading: session.loading,
    sessionError: session.error,
    refreshSession: session.refresh,
    setSession: session.set,
  };
}
