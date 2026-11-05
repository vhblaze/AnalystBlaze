/**
 * Navegação do app (doc 15 §5): estado por chave, sem react-router.
 *
 * Seis telas na sidebar. Meu PC tem abas fixas; Desempenho é a telemetria
 * completa (doc 13 §3). As views atuais ainda falam em "telemetry", "disk" e
 * "network": `fromLegacyView` traduz para a rota nova.
 */

export type ScreenKey = "dashboard" | "history" | "insights" | "controls" | "mypc" | "settings";

export type MyPcTab = "performance" | "disk" | "network" | "health";

/** Detalhes de Controles (doc 13 §4): cada grupo da lista abre o seu. */
export type ControlsSection = "gamemode" | "cleanup" | "startup" | "services" | "protected" | "energy" | "advanced";

export type Route = { screen: ScreenKey; tab?: MyPcTab; section?: ControlsSection };

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

export function route(screen: ScreenKey, sub?: MyPcTab | ControlsSection): Route {
  if (screen === "mypc") return { screen, tab: (sub as MyPcTab | undefined) ?? "performance" };
  if (screen === "controls" && sub) return { screen, section: sub as ControlsSection };
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
  return a.screen === b.screen && a.tab === b.tab && a.section === b.section;
}

/** Nome do evento de navegação (telemetria de uso). */
export function routeId(value: Route): string {
  if (value.screen === "mypc") return `mypc.${value.tab ?? "performance"}`;
  if (value.section) return `${value.screen}.${value.section}`;
  return value.screen;
}

/**
 * Título do header em partes ("CONTROLES › AVANÇADO"). Telas com abas mostram
 * só o nome da tela: a aba ativa já está visível no corpo (doc 14 §1.5).
 */
export function routeTitleKeys(value: Route): string[] {
  const screen = SCREENS.find((item) => item.key === value.screen);
  const keys = [screen?.labelKey ?? "nav.dashboard"];
  if (value.screen === "controls" && value.section) keys.push(`controls.d3.sections.${value.section}.title`);
  return keys;
}
