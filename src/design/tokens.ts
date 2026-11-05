/**
 * Paleta D3 (doc 15 §3) para o que não passa por classe Tailwind: gráficos,
 * treemap, estilos inline. Mesmos valores de tokens.css (tema escuro).
 * Para cor que precisa seguir o tema, prefira `cssVar("accent")`.
 */
export const colors = {
  bg: "#070B14",
  surface: "#0B111D",
  surface2: "#0C1322",
  line: "#172338",
  lineSoft: "#141C2C",
  text: "#E6EDF6",
  text2: "#A9B7CB",
  text3: "#8FA0B6",
  text4: "#6E7F96",
  accent: "#22D3EE",
  accent2: "#38BDF8",
  accentInk: "#06202A",
  violet: "#C4B5FD",
  brandViolet: "#A78BFA",
  ok: "#34D399",
  warn: "#FBBF24",
  danger: "#FCA5A5",
  dangerLine: "#7F1D2D",
} as const;

export type ColorToken =
  | "bg"
  | "surface"
  | "surface-2"
  | "line"
  | "line-soft"
  | "text"
  | "text-2"
  | "text-3"
  | "text-4"
  | "accent"
  | "accent-2"
  | "violet"
  | "ok"
  | "warn"
  | "danger";

/** CSS color that follows the active theme, with optional alpha (0-1). */
export function cssVar(token: ColorToken, alpha = 1): string {
  return alpha >= 1 ? `rgb(var(--${token}-rgb))` : `rgb(var(--${token}-rgb) / ${alpha})`;
}

export const radius = { card: 16, control: 10, pill: 999 } as const;

export const motion = { fastMs: 120, baseMs: 200, easeOut: "cubic-bezier(0.2, 0.8, 0.2, 1)" } as const;
