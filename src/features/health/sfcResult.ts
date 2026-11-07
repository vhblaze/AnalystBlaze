import { useSyncExternalStore } from "react";

/**
 * Quando a última verificação de arquivos do sistema (SFC) deu limpa. O card
 * "Erros críticos nas últimas 24 h" supõe arquivos corrompidos; com um SFC
 * limpo essa hipótese está descartada e o card deixa de ser alerta. Vale 7
 * dias (depois disso a verificação já é velha demais para tranquilizar).
 */
const STORAGE = "analystblaze.health.sfcClean.v1";
export const SFC_CLEAN_VALID_MS = 7 * 24 * 60 * 60 * 1000;

function load(): number | null {
  try {
    const value = Number(localStorage.getItem(STORAGE));
    return Number.isFinite(value) && value > 0 && Date.now() - value < SFC_CLEAN_VALID_MS ? value : null;
  } catch {
    return null;
  }
}

let cleanAt: number | null = load();
const listeners = new Set<() => void>();

export function markSfcClean() {
  cleanAt = Date.now();
  try {
    localStorage.setItem(STORAGE, String(cleanAt));
  } catch {
    // Best effort.
  }
  listeners.forEach((listener) => listener());
}

export function useSfcCleanAt(): number | null {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => cleanAt,
    () => cleanAt,
  );
}
