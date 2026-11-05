import type { NetworkDiagnostics, TracerouteHop, TracerouteResult } from "@/services/tauri/agent";

/**
 * Leituras locais da rede, sem chamar nada: o estado em uma frase, o
 * servidor do jogo "alto" e o veredito do traceroute. Mesmas regras da view
 * antiga (views/Network.tsx), só movidas para cá.
 */

// Mesma régua do card de Insights (GAME_SERVER_LATENCY_GAP_MS/RATIO): a aba
// e o Insight nunca discordam sobre o que é "normal".
const GAME_SERVER_LATENCY_GAP_MS = 60;
const GAME_SERVER_LATENCY_RATIO = 2;

export function isGameServerHigh(diagnostics: NetworkDiagnostics | null): boolean {
  const game = diagnostics?.game_server;
  const internet = diagnostics?.external_latency_ms;
  if (game?.latency_ms == null || internet == null) return false;
  return game.latency_ms - internet >= GAME_SERVER_LATENCY_GAP_MS && game.latency_ms >= internet * GAME_SERVER_LATENCY_RATIO;
}

export type HealthTone = "ok" | "warn" | "danger";

const HEALTH_PRIORITY: Array<{ code: string; tone: HealthTone }> = [
  { code: "network_offline", tone: "danger" },
  { code: "packet_loss_detected", tone: "warn" },
  { code: "jitter_high", tone: "warn" },
  { code: "latency_high", tone: "warn" },
  { code: "wifi_signal_low", tone: "warn" },
  { code: "wifi_legacy_radio", tone: "warn" },
  { code: "vpn_or_virtual_adapter_active", tone: "warn" },
];

/** O estado da rede em uma frase: o código mais grave vira a frase; os outros, o detalhe. */
export function summarizeHealth(recommendations: string[]): { tone: HealthTone; headlineCode: string; detailCodes: string[] } {
  const active = HEALTH_PRIORITY.find((item) => recommendations.includes(item.code));
  if (!active) {
    return { tone: "ok", headlineCode: recommendations.length ? "stable" : "unknown", detailCodes: [] };
  }
  return {
    tone: active.tone,
    headlineCode: active.code,
    detailCodes: recommendations.filter(
      (code) => code !== active.code && code !== "network_stable" && HEALTH_PRIORITY.some((item) => item.code === code),
    ),
  };
}

export const NETWORK_LAG_FLAGS = ["packet_loss_detected", "jitter_high", "latency_high"] as const;
export type NetworkLagFlag = (typeof NETWORK_LAG_FLAGS)[number];

export type RouteVerdict = {
  kind: "local" | "midRoute" | "serverSide" | "normal" | "inconclusive";
  culpritHop: number | null;
  culpritIp: string | null;
};

// Um salto só é o "culpado" se o atraso continua nos saltos seguintes: muitos
// roteadores respondem devagar ao próprio ping (falso positivo clássico do
// traceroute) sem atrasar o tráfego do jogo.
const ROUTE_LOCAL_HOP_THRESHOLD_MS = 35;
const ROUTE_JUMP_THRESHOLD_MS = 25;

export function hopAverageRtt(hop: TracerouteHop): number | null {
  const values = hop.rttMs.filter((value): value is number => value != null);
  if (values.length === 0) return null;
  return values.reduce((sum, value) => sum + value, 0) / values.length;
}

/** Interpretação local do traceroute: "é a minha rede, o caminho ou o servidor?". Heurística, não certeza. */
export function classifyRouteDiagnosis(trace: TracerouteResult): RouteVerdict {
  const responsive = trace.hops
    .map((hop) => ({ hop: hop.hop, ip: hop.ip ?? null, rtt: hopAverageRtt(hop) }))
    .filter((entry): entry is { hop: number; ip: string | null; rtt: number } => entry.rtt != null);

  if (responsive.length < 2) return { kind: "inconclusive", culpritHop: null, culpritIp: null };

  const first = responsive[0];
  if (first.rtt >= ROUTE_LOCAL_HOP_THRESHOLD_MS) return { kind: "local", culpritHop: first.hop, culpritIp: first.ip };

  let best: { index: number; jump: number } | null = null;
  for (let i = 1; i < responsive.length; i += 1) {
    const jump = responsive[i].rtt - responsive[i - 1].rtt;
    if (jump >= ROUTE_JUMP_THRESHOLD_MS && (best == null || jump > best.jump)) best = { index: i, jump };
  }
  if (!best) return { kind: "normal", culpritHop: null, culpritIp: null };

  const bestIndex = best.index;
  const sticks = responsive.slice(bestIndex).every((entry) => entry.rtt >= responsive[bestIndex].rtt - ROUTE_JUMP_THRESHOLD_MS / 2);
  if (!sticks) return { kind: "normal", culpritHop: null, culpritIp: null };

  const culprit = responsive[bestIndex];
  return {
    kind: bestIndex === responsive.length - 1 ? "serverSide" : "midRoute",
    culpritHop: culprit.hop,
    culpritIp: culprit.ip,
  };
}

export function errorMessage(error: unknown) {
  if (error instanceof Error) return error.message;
  return String(error);
}
