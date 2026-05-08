import type { TFunction } from "@/types/i18n";

export type InsightCategory = "performance" | "energia" | "rede" | "limpeza";

export type Insight = {
  title: string;
  explanation: string;
  impact: string;
  category: InsightCategory;
};

export type InsightResult = {
  insights: Insight[];
  source: "remote" | "local";
};

export async function fetchInsights(t: TFunction): Promise<InsightResult> {
  const url = import.meta.env.VITE_ANALYSTBLAZE_INSIGHTS_URL?.trim();
  if (!url) {
    return { insights: localInsights(t), source: "local" };
  }

  const response = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ source: "analystblaze-desktop" }),
  });

  if (!response.ok) {
    throw new Error(`Insights endpoint returned ${response.status}`);
  }

  const data = await response.json();
  return {
    insights: (data?.insights ?? []) as Insight[],
    source: "remote",
  };
}

function localInsights(t: TFunction): Insight[] {
  return [
    {
      title: t("insights.fallback.performanceTitle"),
      explanation: t("insights.fallback.performanceText"),
      impact: t("insights.fallback.performanceImpact"),
      category: "performance",
    },
    {
      title: t("insights.fallback.cleanupTitle"),
      explanation: t("insights.fallback.cleanupText"),
      impact: t("insights.fallback.cleanupImpact"),
      category: "limpeza",
    },
    {
      title: t("insights.fallback.energyTitle"),
      explanation: t("insights.fallback.energyText"),
      impact: t("insights.fallback.energyImpact"),
      category: "energia",
    },
    {
      title: t("insights.fallback.networkTitle"),
      explanation: t("insights.fallback.networkText"),
      impact: t("insights.fallback.networkImpact"),
      category: "rede",
    },
  ];
}
