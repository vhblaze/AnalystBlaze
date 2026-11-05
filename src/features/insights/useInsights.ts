import { fetchInsights, type Insight } from "@/services/insights";
import type { Locale } from "@/i18n";
import { createAgentResource } from "@/features/shared/agentResource";

/**
 * Sugestões do servidor: a mesma leitura na tela de Insights, no ponto de
 * novidade da sidebar e no aviso "sugestão nova" (doc 14 U7).
 */
export const serverInsights = createAgentResource<Insight[], [Locale]>({
  name: "insights.server",
  fetch: async (locale) => (await fetchInsights(() => "", locale)).insights,
  staleMs: 30 * 60_000,
});

const SEEN_STORAGE = "analystblaze.insights.seen.v1";

/** Chave estável de uma sugestão (o servidor não manda id). */
export function insightKey(insight: Insight): string {
  return `${insight.category}:${insight.title}`.toLowerCase();
}

export function readSeenInsights(): Set<string> {
  try {
    const raw = JSON.parse(localStorage.getItem(SEEN_STORAGE) ?? "[]");
    return new Set(Array.isArray(raw) ? raw.filter((v) => typeof v === "string") : []);
  } catch {
    return new Set();
  }
}

export function markInsightsSeen(insights: Insight[]) {
  try {
    const seen = readSeenInsights();
    insights.forEach((insight) => seen.add(insightKey(insight)));
    localStorage.setItem(SEEN_STORAGE, JSON.stringify(Array.from(seen).slice(-200)));
  } catch {
    // Sem localStorage: tudo conta como novo da próxima vez.
  }
}

/** Sugestões que o usuário ainda não viu na tela de Insights. */
export function unseenInsights(insights: Insight[] | undefined): Insight[] {
  const seen = readSeenInsights();
  return (insights ?? []).filter((insight) => !seen.has(insightKey(insight)));
}
