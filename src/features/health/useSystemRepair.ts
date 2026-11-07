import { useSyncExternalStore } from "react";
import {
  getDismRestoreHealthStatus,
  getSystemFileCheckStatus,
  type OptimizationResult,
  type SystemFileCheckStatus,
} from "@/services/tauri/agent";
import { errorText } from "@/features/shared/agentResource";
import { markSfcClean } from "./sfcResult";

/**
 * Reparo do Windows (SFC e DISM). A varredura leva minutos e o helper não
 * empurra progresso, então o app consulta a cada 5 s (system_repair.rs).
 * O estado mora aqui, fora da tela: trocar de tela não perde a varredura.
 */

export type RepairKind = "sfc" | "dism";

export type RepairState = {
  scanId: string | null;
  starting: boolean;
  status: SystemFileCheckStatus | null;
  error: string | null;
};

const POLL_MS = 5_000;

/** Qual reparo está com o pop-up aberto (null = nenhum). Abre quando a pessoa confirma e a varredura começa. */
let panelKind: RepairKind | null = null;
function openPanel(kind: RepairKind) {
  panelKind = kind;
  listeners.forEach((listener) => listener());
}
export function closeRepairPanel() {
  panelKind = null;
  listeners.forEach((listener) => listener());
}
export function useRepairPanel(): RepairKind | null {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => panelKind,
    () => panelKind,
  );
}
const empty: RepairState = { scanId: null, starting: false, status: null, error: null };
let state: Record<RepairKind, RepairState> = { sfc: empty, dism: empty };
const listeners = new Set<() => void>();
const timers: Partial<Record<RepairKind, number>> = {};

const pollers: Record<RepairKind, (scanId: string) => Promise<OptimizationResult>> = {
  sfc: getSystemFileCheckStatus,
  dism: getDismRestoreHealthStatus,
};

function patch(kind: RepairKind, next: Partial<RepairState>) {
  state = { ...state, [kind]: { ...state[kind], ...next } };
  listeners.forEach((listener) => listener());
}

function stopPolling(kind: RepairKind) {
  if (timers[kind] !== undefined) window.clearInterval(timers[kind]);
  delete timers[kind];
}

async function poll(kind: RepairKind) {
  const scanId = state[kind].scanId;
  if (!scanId) return stopPolling(kind);
  try {
    const result = await pollers[kind](scanId);
    if (!result.success) {
      patch(kind, { scanId: null, error: result.message });
      return stopPolling(kind);
    }
    const status = result.details as unknown as SystemFileCheckStatus;
    patch(kind, { status });
    if (status.done) {
      if (kind === "sfc" && status.result?.outcome === "clean") markSfcClean();
      patch(kind, { scanId: null });
      stopPolling(kind);
    }
  } catch (error) {
    patch(kind, { scanId: null, error: errorText(error) });
    stopPolling(kind);
  }
}

/**
 * Começa uma varredura. `start` já pede a confirmação e devolve `false`
 * quando a pessoa cancela.
 */
export async function startRepair(kind: RepairKind, start: () => Promise<unknown>): Promise<void> {
  if (state[kind].scanId || state[kind].starting) return;
  patch(kind, { starting: true, error: null });
  try {
    const result = await start();
    if (result === false || !result || typeof result !== "object") return;
    const outcome = result as OptimizationResult;
    if (!outcome.success) {
      patch(kind, { error: outcome.message });
      openPanel(kind);
      return;
    }
    const scanId = (outcome.details as { scanId?: string } | null)?.scanId;
    if (!scanId) return;
    patch(kind, { scanId, status: null });
    openPanel(kind);
    void poll(kind);
    stopPolling(kind);
    timers[kind] = window.setInterval(() => void poll(kind), POLL_MS);
  } catch (error) {
    patch(kind, { error: errorText(error) });
  } finally {
    patch(kind, { starting: false });
  }
}

export function useSystemRepair(): Record<RepairKind, RepairState> {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => state,
    () => state,
  );
}
