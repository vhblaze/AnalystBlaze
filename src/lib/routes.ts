/**
 * Navegação do app (doc 15 §5): estado por chave, sem react-router.
 *
 * Seis telas na sidebar. Meu PC tem abas fixas; Desempenho é a telemetria
 * completa (doc 13 §3). As views atuais ainda falam em "telemetry", "disk" e
 * "network": `fromLegacyView` traduz para a rota nova.
 */

export type ScreenKey = "dashboard" | "history" | "insights" | "controls" | "mypc" | "settings";

export type MyPcTab = "performance" | "disk" | "network" | "health";

export type Route = { screen: ScreenKey; tab?: MyPcTab };

/** Chaves das telas antigas (v0.1.77), usadas por views e atalhos que ainda não mudaram. */
export type LegacyView = "dashboard" | "telemetry" | "insights" | "controls" | "disk" | "network" | "settings";

export const SCREENS: ReadonlyArray<{ key: ScreenKey; labelKey: string }> = [
  { key: "dashboard", labelKey: "nav.dashboard" },
  { key: "history", labelKey: "nav.history" },
  { key: "insights", labelKey: "nav.insights" },
  { key: "controls", labelKey: "nav.controls" },
  { key: "mypc", labelKey: "nav.mypc" },
  { key: "settings", labelKey: "nav.settings" },
];

export const MYPC_TABS: ReadonlyArray<{ key: MyPcTab; labelKey: string }> = [
  { key: "performance", labelKey: "mypc.tabs.performance" },
  { key: "disk", labelKey: "mypc.tabs.disk" },
  { key: "network", labelKey: "mypc.tabs.network" },
  { key: "health", labelKey: "mypc.tabs.health" },
];

export const DEFAULT_ROUTE: Route = { screen: "dashboard" };

export function route(screen: ScreenKey, tab?: MyPcTab): Route {
  if (screen === "mypc") return { screen, tab: tab ?? "performance" };
  return { screen };
}

export function fromLegacyView(view: LegacyView): Route {
  switch (view) {
    case "telemetry":
      return route("mypc", "performance");
    case "disk":
      return route("mypc", "disk");
    case "network":
      return route("mypc", "network");
    default:
      return route(view);
  }
}

export function sameRoute(a: Route, b: Route): boolean {
  return a.screen === b.screen && (a.screen !== "mypc" || a.tab === b.tab);
}

/** Nome do evento de navegação (telemetria de uso). */
export function routeId(value: Route): string {
  return value.screen === "mypc" ? `mypc.${value.tab ?? "performance"}` : value.screen;
}

/**
 * Título do header em partes ("CONTROLES › AVANÇADO"). Telas com abas mostram
 * só o nome da tela: a aba ativa já está visível no corpo (doc 14 §1.5).
 */
export function routeTitleKeys(value: Route): string[] {
  const screen = SCREENS.find((item) => item.key === value.screen);
  return [screen?.labelKey ?? "nav.dashboard"];
}
