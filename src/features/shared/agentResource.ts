import { useCallback, useEffect, useSyncExternalStore } from "react";

/**
 * Uma fonte de verdade por comando do agente (doc 15 §2, doc 14 Q1–Q3).
 *
 * Cada recurso guarda o último valor lido em um store de módulo: duas telas
 * que mostram o mesmo dado leem o mesmo estado, nunca duas chamadas que
 * podem responder diferente. Uma leitura em andamento é reaproveitada por
 * quem pedir de novo, e eventos do agente (ex.: uso semanal do Modo Jogo)
 * atualizam o mesmo store.
 */

export type ResourceState<T> = {
  data: T | undefined;
  error: string | null;
  loading: boolean;
  /** Quando o valor foi lido (ms). null = nunca leu. */
  updatedAt: number | null;
};

type ResourceOptions<T, Args extends unknown[]> = {
  name: string;
  fetch: (...args: Args) => Promise<T>;
  /** Assinatura de evento que empurra valores novos; ativa enquanto houver tela usando o recurso. */
  subscribe?: (push: (value: T) => void) => Promise<() => void>;
  /** Idade máxima para reaproveitar o valor ao abrir uma tela (padrão: 30 s). */
  staleMs?: number;
};

export function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

export type AgentResource<T, Args extends unknown[]> = {
  name: string;
  getState: () => ResourceState<T>;
  /** Lê de novo (sempre). Leitura já em andamento é reaproveitada. */
  refresh: (...args: Args) => Promise<T>;
  /** Grava um valor conhecido (resultado de uma ação, evento do agente). */
  set: (value: T | undefined) => void;
  use: (options?: { auto?: boolean; args?: Args }) => ResourceState<T> & {
    refresh: (...args: Args) => Promise<T>;
    set: (value: T | undefined) => void;
  };
};

export function createAgentResource<T, Args extends unknown[] = []>(
  options: ResourceOptions<T, Args>,
): AgentResource<T, Args> {
  const staleMs = options.staleMs ?? 30_000;
  let state: ResourceState<T> = { data: undefined, error: null, loading: false, updatedAt: null };
  let inflight: Promise<T> | null = null;
  const listeners = new Set<() => void>();
  let users = 0;
  let unsubscribeEvents: (() => void) | null = null;
  let subscribing = false;

  const emit = (patch: Partial<ResourceState<T>>) => {
    state = { ...state, ...patch };
    listeners.forEach((listener) => listener());
  };

  const refresh = (...args: Args): Promise<T> => {
    if (inflight) return inflight;
    emit({ loading: true, error: null });
    const request = options
      .fetch(...args)
      .then(
        (value) => {
          emit({ data: value, loading: false, error: null, updatedAt: Date.now() });
          return value;
        },
        (error: unknown) => {
          emit({ loading: false, error: errorText(error) });
          throw error;
        },
      )
      .finally(() => {
        if (inflight === request) inflight = null;
      });
    inflight = request;
    return request;
  };

  const set = (value: T | undefined) => emit({ data: value, error: null, updatedAt: Date.now() });

  const subscribeStore = (listener: () => void) => {
    listeners.add(listener);
    return () => listeners.delete(listener);
  };

  const attachEvents = () => {
    users += 1;
    if (!options.subscribe || unsubscribeEvents || subscribing) return;
    subscribing = true;
    options
      .subscribe((value) => set(value))
      .then((dispose) => {
        subscribing = false;
        if (users > 0) unsubscribeEvents = dispose;
        else dispose();
      })
      .catch(() => {
        subscribing = false;
      });
  };

  const detachEvents = () => {
    users = Math.max(0, users - 1);
    if (users === 0 && unsubscribeEvents) {
      unsubscribeEvents();
      unsubscribeEvents = null;
    }
  };

  function use({ auto = true, args }: { auto?: boolean; args?: Args } = {}) {
    const snapshot = useSyncExternalStore(subscribeStore, () => state, () => state);

    useEffect(() => {
      attachEvents();
      return detachEvents;
    }, []);

    useEffect(() => {
      if (!auto) return;
      const fresh = state.updatedAt !== null && Date.now() - state.updatedAt < staleMs;
      if (!fresh && !inflight) {
        void refresh(...((args ?? []) as Args)).catch(() => undefined);
      }
      // args só valem para a primeira leitura; mudanças pedem refresh explícito.
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [auto]);

    const stableRefresh = useCallback((...next: Args) => refresh(...next), []);
    return { ...snapshot, refresh: stableRefresh, set };
  }

  return { name: options.name, getState: () => state, refresh, set, use };
}
