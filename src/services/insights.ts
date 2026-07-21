import {
  fetchAuthenticatedInsights,
  isTauriRuntime,
} from "@/services/tauri/agent";
import type { TFunction } from "@/types/i18n";
import type { Locale } from "@/i18n";

export type InsightCategory = "performance" | "energia" | "rede" | "limpeza";

export type Insight = {
  title: string;
  explanation: string;
  impact: string;
  category: InsightCategory;
  /** Only set for insights computed locally (not from the backend) that
   * link to more detail elsewhere in the app - e.g. the disk-usage card
   * linking to Local Controls. */
  action?: { label: string; onClick: () => void };
};

export type InsightResult = {
  insights: Insight[];
  source: "remote";
};

type ServerCard = {
  id?: string;
  title?: string;
  message?: string;
  recommendation?: string | null;
  severity?: string;
  metrics?: Record<string, unknown>;
};

type ServerAction = {
  actionName?: string;
  title?: string;
  reason?: string;
  confidence?: number;
  expectedImpact?: string;
  metrics?: Record<string, unknown>;
};

type ServerInsightsResponse = {
  cards?: ServerCard[];
  recommendedActions?: ServerAction[];
  source?: string;
};

export async function fetchInsights(_t: TFunction, locale: Locale): Promise<InsightResult> {
  if (!isTauriRuntime()) {
    throw new Error(serverOnlyInsightsMessage(locale));
  }

  try {
    const payload = await fetchAuthenticatedInsights(locale);
    return { insights: normalizeServerInsights(payload), source: "remote" };
  } catch (error) {
    const message = error instanceof Error && error.message ? error.message : serverOnlyInsightsMessage(locale);
    throw new Error(message);
  }
}

function normalizeServerInsights(payload: unknown): Insight[] {
  if (!isRecord(payload)) return [];
  const response = payload as ServerInsightsResponse;
  const cards = Array.isArray(response.cards) ? response.cards : [];
  const actions = Array.isArray(response.recommendedActions) ? response.recommendedActions : [];

  const cardInsights = cards.map((card, index) => {
    const pairedAction = actions[index];
    return {
      title: card.title || pairedAction?.title || "Insight",
      explanation: [card.message, card.recommendation].filter(Boolean).join(" ") || pairedAction?.reason || "",
      impact: impactFrom(card.metrics, pairedAction),
      category: categoryFrom(card, pairedAction),
    };
  });

  const actionOnlyInsights = actions
    .slice(cards.length)
    .map((action) => ({
      title: action.title || action.actionName || "Acao recomendada",
      explanation: action.reason || "",
      impact: action.expectedImpact || confidenceImpact(action.confidence),
      category: categoryFrom(undefined, action),
    }));

  return [...cardInsights, ...actionOnlyInsights].filter(
    (insight) => insight.title.trim() || insight.explanation.trim(),
  );
}

function textFor(locale: Locale, pt: string, en: string) {
  return locale === "pt-BR" ? pt : en;
}

function serverOnlyInsightsMessage(locale: Locale) {
  return textFor(
    locale,
    "Insights sao gerados apenas pelo backend server. Faca login e mantenha a API configurada para receber a analise.",
    "Insights are generated only by the backend server. Sign in and keep the API configured to receive the analysis.",
  );
}

function categoryFrom(card?: ServerCard, action?: ServerAction): InsightCategory {
  const text = `${card?.id ?? ""} ${card?.title ?? ""} ${action?.actionName ?? ""} ${action?.title ?? ""}`.toLowerCase();
  if (text.includes("network") || text.includes("latency") || text.includes("rede")) return "rede";
  if (text.includes("power") || text.includes("energy") || text.includes("battery") || text.includes("energia")) return "energia";
  if (text.includes("temp") || text.includes("cleanup") || text.includes("disk") || text.includes("limpeza")) return "limpeza";
  return "performance";
}

function impactFrom(metrics?: Record<string, unknown>, action?: ServerAction) {
  const expected = stringValue(metrics?.expectedImpact) ?? stringValue(metrics?.expected_impact);
  return expected ?? action?.expectedImpact ?? confidenceImpact(action?.confidence);
}

function confidenceImpact(confidence?: number) {
  if (confidence == null) return "impacto calculado pelo backend";
  return `${Math.round(confidence * 100)}% de confianca`;
}

function stringValue(value: unknown) {
  return typeof value === "string" && value.trim() ? value : null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value && typeof value === "object" && !Array.isArray(value));
}
