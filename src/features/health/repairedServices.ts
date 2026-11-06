import { useSyncExternalStore } from "react";

/**
 * Serviços que o usuário mandou "Tentar corrigir" e deram certo. O aviso vem
 * do log do Windows (últimos 7 dias), que não apaga o passado: sem isto, o
 * serviço religado continuaria na lista. Guardamos quantas falhas o log tinha
 * na hora; o aviso só volta se o número crescer (falha nova) ou em 7 dias.
 */
const STORAGE = "analystblaze.health.repaired.v1";
const TTL_MS = 7 * 24 * 60 * 60 * 1000;

type Entry = { count: number; at: number };
export type RepairedServices = Record<string, Entry>;

function load(): RepairedServices {
  try {
    const raw = JSON.parse(localStorage.getItem(STORAGE) ?? "{}") as RepairedServices;
    const now = Date.now();
    return Object.fromEntries(Object.entries(raw).filter(([, entry]) => entry && now - entry.at < TTL_MS));
  } catch {
    return {};
  }
}

let state: RepairedServices = load();
const listeners = new Set<() => void>();

export function markServiceRepaired(name: string, count: number) {
  state = { ...state, [name.toLowerCase()]: { count, at: Date.now() } };
  try {
    localStorage.setItem(STORAGE, JSON.stringify(state));
  } catch {
    // Best effort.
  }
  listeners.forEach((listener) => listener());
}

export function useRepairedServices(): RepairedServices {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => state,
  );
}

/** O serviço foi corrigido e não falhou de novo desde então? */
export function isRepaired(repaired: RepairedServices | undefined, name: string, count: number | undefined): boolean {
  const entry = repaired?.[name.toLowerCase()];
  return Boolean(entry) && (count ?? 0) <= entry.count;
}
