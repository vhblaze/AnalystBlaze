import {
  getActiveGameModeSession,
  getForegroundGame,
  getFrameMeasurements,
  type ForegroundGame,
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

/** Jogo aberto em primeiro plano, com ou sem Modo Jogo. Releitura a cada 15 s. */
export const foregroundGame = createAgentResource<ForegroundGame>({
  name: "gamemode.foregroundGame",
  fetch: getForegroundGame,
  staleMs: 15_000,
});

/** Ganho médio no 1% low (em %) entre as medições que deram número, ou null. */
export function averageLowGainPercent(entries: FrameMeasurementEntry[] | undefined): { percent: number; count: number } | null {
  const valid = (entries ?? [])
    .map((entry) => entry.measurement)
    .filter((m) => m.verdict && m.verdict !== "incomplete" && m.baseline?.low1PctFps && m.deltaLow1PctFps != null);
  if (valid.length === 0) return null;
  const sum = valid.reduce((acc, m) => acc + ((m.deltaLow1PctFps ?? 0) / (m.baseline?.low1PctFps ?? 1)) * 100, 0);
  return { percent: sum / valid.length, count: valid.length };
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
