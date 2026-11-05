// Textos em inglês · insights. Mesmas chaves nas duas línguas (o tipo Mirror garante).
// Gerado na etapa 7 do doc 15 a partir do translations.ts antigo; edite à vontade.
import type { Mirror } from "../mirror";
import type { insights as pt } from "../pt/insights";

export const insights: Mirror<typeof pt> = {
  insights: {
    errorFallback: "Failed to fetch insights from the backend",
    errorTitle: "Couldn't load insights",
    errorRetry: "Try again",
    openInWeb: "Open on web",
    categories: {
      performance: "performance",
      energy: "energy",
      network: "network",
      cleanup: "cleanup",
    },
  },
  insightsD3: {
    title: "{{count}} things to improve",
    titleNone: "Nothing to improve right now",
    subtitle: "Suggestions from what the agent measures on this PC. The most important comes first.",
    loading: "Reading suggestions…",
    emptyTitle: "No suggestions right now",
    emptyReason: "When the agent measures something that can be improved, it shows up here with the reason.",
    letDo: "Let AnalystBlaze do it",
    doNow: "Do it now",
    doMyself: "I'll do it myself",
    disable: "Disable",
    notNow: "Not now",
    showAll: "See all ({{count}} more)",
    showLess: "Show only the top 3",
    reversible: "can be undone",
    notReversible: "cannot be undone",
    risk: {
      baixo: "Low risk",
      medio: "Medium risk",
      alto: "High risk",
    },
    healthLabel: "Windows health",
    healthLabelCount: "Windows health · {{count}} warnings",
    healthOk: "No Windows warnings in the latest reading.",
    healthNote: "These are not performance suggestions: they live in My PC.",
    healthOpen: "See in My PC › Windows health",
    howLabel: "How suggestions work",
    how: {
      max: "At most 3 at a time; the rest are under \"see all\".",
      notNow: "\"Not now\" hides the suggestion for 24 hours.",
      nothingAlone: "Nothing changes unless you ask or an automation you turned on does it.",
      history: "What AnalystBlaze does goes to History, with Undo.",
    },
    silenced: "Show the {{count}} hidden again",
  },
};
