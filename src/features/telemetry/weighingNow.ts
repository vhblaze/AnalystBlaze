import type { AgentTelemetrySnapshot, DiskVolumeInfo, ForegroundGame, GameModeSession } from "@/services/tauri/agent";

/**
 * "O que está pesando agora" (doc 13 §4, doc 11 §0): o maior problema da
 * última leitura, em uma frase, com uma ação. Regras fixas e explícitas:
 * nada estimado, e nada que o agente não mediu (sem "o Chrome usa 9,6 GB"
 * até existir memória por processo, lacuna L1).
 */

export const THRESHOLDS = {
  memoryPercent: 85,
  cpuPercent: 90,
  diskFreePercent: 10,
  diskFreeBytes: 15 * 1024 ** 3,
  latencyMs: 120,
} as const;

export type WeighingNow =
  | { kind: "game_mode"; game: string | null }
  | { kind: "game_open"; game: string }
  | { kind: "memory"; percent: number; topProcess: { name: string; memoryMb: number } | null }
  | { kind: "disk"; mountPoint: string; freeBytes: number }
  | { kind: "cpu"; percent: number }
  | { kind: "network"; latencyMs: number }
  | { kind: "ok" };

/** Unidade do Windows (C:\) ou, sem ela, a maior. */
export function systemVolume(volumes: DiskVolumeInfo[]): DiskVolumeInfo | null {
  const system = volumes.find((volume) => /^c:/i.test(volume.mountPoint));
  if (system) return system;
  return [...volumes].filter((volume) => !volume.isRemovable).sort((a, b) => b.totalBytes - a.totalBytes)[0] ?? null;
}

export function pickWeighingNow({
  telemetry,
  volumes,
  gameSession,
  foregroundGame = null,
  dismissed = [],
}: {
  telemetry: AgentTelemetrySnapshot | null;
  volumes: DiskVolumeInfo[];
  gameSession: GameModeSession | null;
  /** Jogo em primeiro plano agora, mesmo sem Modo Jogo (doc 14 U2). */
  foregroundGame?: ForegroundGame | null;
  /** Tipos que o usuário dispensou nesta sessão ("Agora não"). */
  dismissed?: WeighingNow["kind"][];
}): WeighingNow {
  const allowed = (kind: WeighingNow["kind"]) => !dismissed.includes(kind);

  // Doc 14 U2: com o Modo Jogo ligado, o card fala do jogo.
  if (gameSession && allowed("game_mode")) {
    return { kind: "game_mode", game: gameSession.targetProcessName ?? null };
  }
  // Doc 14 U2: jogo aberto sem Modo Jogo também fala do jogo.
  if (foregroundGame?.detected && foregroundGame.process_name && allowed("game_open")) {
    return { kind: "game_open", game: foregroundGame.process_name };
  }
  if (!telemetry) return { kind: "ok" };

  const memory = telemetry.ram_usage_percent;
  if (typeof memory === "number" && memory >= THRESHOLDS.memoryPercent && allowed("memory")) {
    const top = telemetry.top_processes?.[0];
    return {
      kind: "memory",
      percent: memory,
      topProcess: top ? { name: top.name, memoryMb: top.memory_mb } : null,
    };
  }

  const volume = systemVolume(volumes);
  if (volume && volume.totalBytes > 0 && allowed("disk")) {
    const freePercent = (volume.availableBytes / volume.totalBytes) * 100;
    if (freePercent < THRESHOLDS.diskFreePercent || volume.availableBytes < THRESHOLDS.diskFreeBytes) {
      return { kind: "disk", mountPoint: volume.mountPoint, freeBytes: volume.availableBytes };
    }
  }

  if (telemetry.cpu_usage >= THRESHOLDS.cpuPercent && allowed("cpu")) {
    return { kind: "cpu", percent: telemetry.cpu_usage };
  }

  if (telemetry.latency_ms >= THRESHOLDS.latencyMs && allowed("network")) {
    return { kind: "network", latencyMs: telemetry.latency_ms };
  }

  return { kind: "ok" };
}
