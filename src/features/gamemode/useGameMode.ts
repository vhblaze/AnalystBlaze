import {
  getActiveGameModeSession,
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
