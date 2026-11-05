import { useEffect, useSyncExternalStore } from "react";
import { listenToAgentTelemetry, type AgentTelemetrySample } from "@/services/tauri/agent";
import { liveTelemetry } from "@/features/telemetry/useLiveTelemetry";

/**
 * Histórico curto da telemetria (até 1 hora) guardado em módulo, fora de
 * qualquer tela: trocar de aba não apaga o gráfico de Meu PC › Desempenho.
 *
 * Fonte: o mesmo evento "telemetry-update" que alimenta `liveTelemetry`
 * (um ouvinte próprio, porque o store de `liveTelemetry` só escuta enquanto
 * alguma tela o usa e o histórico precisa continuar enchendo fora da aba).
 * As amostras de "Coletar agora" também entram por `recordTelemetrySample`.
 *
 * Nada sai do PC: fica só em memória e some quando o app fecha.
 */

export const HISTORY_MAX_SECONDS = 60 * 60;

export type TelemetryMetricKey = "cpu" | "gpu" | "ram" | "disk" | "latency" | "gpuTemp" | "vram" | "cpuTemp";

export type TelemetryPoint = {
  /** Unix em segundos (event_timestamp do agente). */
  t: number;
  values: Record<TelemetryMetricKey, number | null>;
};

function finite(value: number | null | undefined): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

/** Lê os 8 indicadores de uma amostra; null = o agente não tem leitura. */
export function readTelemetryValues(sample: AgentTelemetrySample): Record<TelemetryMetricKey, number | null> {
  return {
    cpu: finite(sample.cpu_usage),
    gpu: sample.gpu_usage_available ? finite(sample.gpu_usage) : null,
    ram: finite(sample.ram_usage_percent),
    disk: finite(sample.disk_usage_percent),
    latency: sample.latency_ms && sample.latency_ms > 0 ? finite(sample.latency_ms) : null,
    gpuTemp:
      sample.gpu_temperature_available && sample.gpu_temperature > 0 ? finite(sample.gpu_temperature) : null,
    vram: finite(sample.vram_usage_percent),
    cpuTemp:
      sample.cpu_temperature_available && (sample.cpu_temperature ?? 0) > 0 ? finite(sample.cpu_temperature) : null,
  };
}

type HistoryState = {
  points: TelemetryPoint[];
  /** Quando o registro começou (Unix em segundos). null = ainda não começou. */
  startedAt: number | null;
};

let state: HistoryState = { points: [], startedAt: null };
const listeners = new Set<() => void>();
let started = false;

function emit(next: HistoryState) {
  state = next;
  listeners.forEach((listener) => listener());
}

/** Junta uma amostra ao histórico. Mesma amostra (mesmo segundo) substitui; mais velha que a última é ignorada. */
export function recordTelemetrySample(sample: AgentTelemetrySample | null | undefined) {
  if (!sample || !Number.isFinite(sample.event_timestamp)) return;
  const point: TelemetryPoint = { t: sample.event_timestamp, values: readTelemetryValues(sample) };
  const last = state.points[state.points.length - 1];
  let points: TelemetryPoint[];
  if (last && point.t < last.t) return;
  if (last && point.t === last.t) {
    points = [...state.points.slice(0, -1), point];
  } else {
    points = [...state.points, point];
  }
  const cutoff = point.t - HISTORY_MAX_SECONDS;
  const firstKept = points.findIndex((item) => item.t >= cutoff);
  if (firstKept > 0) points = points.slice(firstKept);
  emit({ points, startedAt: state.startedAt ?? Math.floor(Date.now() / 1000) });
}

/**
 * Começa a registrar (idempotente). Chamar no início do app faz o histórico
 * valer "desde que o app abriu"; a tela Desempenho também chama ao montar.
 */
export function startTelemetryHistory() {
  if (started) return;
  started = true;
  if (state.startedAt === null) emit({ ...state, startedAt: Math.floor(Date.now() / 1000) });
  recordTelemetrySample(liveTelemetry.getState().data ?? null);
  void listenToAgentTelemetry((snapshot) => recordTelemetrySample(snapshot)).catch(() => {
    started = false;
  });
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getTelemetryHistory(): HistoryState {
  return state;
}

/** Histórico de até 1 hora, compartilhado por todas as telas. */
export function useTelemetryHistory(): HistoryState {
  useEffect(() => {
    startTelemetryHistory();
  }, []);
  return useSyncExternalStore(subscribe, getTelemetryHistory, getTelemetryHistory);
}
