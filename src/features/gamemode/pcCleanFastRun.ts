import { useSyncExternalStore } from "react";

/**
 * Estado do pop-up "PC limpo e rápido". O comando do agente é uma chamada só
 * (mede, limpa, ajusta, mede de novo), sem eventos de andamento: o pop-up
 * mostra o que a regra faz e o tempo, e o resultado vem do que o agente mediu.
 */
export type PcCleanFastRunState =
  | { phase: "idle" }
  | { phase: "running"; startedAt: number }
  | { phase: "done"; before: number | null; after: number | null; steps: string[]; sessionId: string | null }
  | { phase: "failed"; message: string };

let state: PcCleanFastRunState = { phase: "idle" };
const listeners = new Set<() => void>();

function set(next: PcCleanFastRunState) {
  state = next;
  listeners.forEach((listener) => listener());
}

export function usePcCleanFastRun(): PcCleanFastRunState {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => state,
  );
}

export function closePcCleanFastRun() {
  if (state.phase !== "running") set({ phase: "idle" });
}

type Details = {
  baselineReport?: { overallScore?: number };
  afterReport?: { overallScore?: number };
  session?: { id?: string; actions?: { actionName?: string; status?: string }[] };
};

function readResult(result: unknown): PcCleanFastRunState {
  const details = ((result as { details?: unknown } | null)?.details ?? {}) as Details;
  const score = (value: number | undefined) => (typeof value === "number" && Number.isFinite(value) ? Math.round(value) : null);
  const steps = (details.session?.actions ?? [])
    .filter((action) => action.status !== "skipped" && action.status !== "failed" && action.actionName)
    .map((action) => action.actionName as string);
  return {
    phase: "done",
    before: score(details.baselineReport?.overallScore),
    after: score(details.afterReport?.overallScore),
    steps,
    sessionId: details.session?.id ?? null,
  };
}

/** Roda o perfil mostrando o pop-up. Devolve o mesmo resultado do comando. */
export async function runPcCleanFastWithProgress(action: () => Promise<unknown>): Promise<unknown> {
  set({ phase: "running", startedAt: Date.now() });
  try {
    const result = await action();
    set(readResult(result));
    return result;
  } catch (error) {
    set({ phase: "failed", message: error instanceof Error ? error.message : String(error) });
    throw error;
  }
}
