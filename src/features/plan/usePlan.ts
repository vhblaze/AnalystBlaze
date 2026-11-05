import { canUseAutomaticGameMode, canUsePaidGameMode } from "@/hooks/useAuth";
import type { AgentStatus } from "@/services/tauri/agent";

/**
 * O que o plano libera, num lugar só (doc 15 §6). As telas leem daqui em vez
 * de repetir a regra; o motivo de um bloqueio também vem daqui.
 */
export function usePlan(status?: AgentStatus | null) {
  const paidGameMode = canUsePaidGameMode(status);
  const automaticGameMode = canUseAutomaticGameMode(status);
  return {
    /** Modo Jogo sem limite semanal. */
    paidGameMode,
    /** Modo Jogo que liga sozinho ao abrir um jogo. */
    automaticGameMode,
    /** Starter: Modo Jogo limitado por semana e automações com cadeado. */
    limited: !paidGameMode,
  };
}
