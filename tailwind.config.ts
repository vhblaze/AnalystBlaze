import type { Config } from "tailwindcss";
import tailwindcssAnimate from "tailwindcss-animate";

// Paleta D3 (doc 15 §3). Os valores moram em src/design/tokens.css; aqui só
// se dá nome a eles. As classes de paleta do Tailwind (slate, cyan, amber,
// rose, emerald, violet, sky...) não devem ser usadas: cada cor do app passa
// por um destes nomes.
const token = (name: string) => `rgb(var(--${name}-rgb) / <alpha-value>)`;

const tone = (name: string) => ({
  DEFAULT: token(name),
  soft: token(`${name}-soft`),
  strong: token(`${name}-strong`),
});

export default {
  darkMode: ["class"],
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  prefix: "",
  theme: {
    container: {
      center: true,
      padding: "2rem",
      screens: {
        "2xl": "1400px",
      },
    },
    // Fora do "extend" de propósito: a paleta padrão do Tailwind deixa de
    // existir, então uma classe solta (text-slate-400) não gera CSS.
    colors: {
      transparent: "transparent",
      current: "currentColor",
      inherit: "inherit",
      white: "#ffffff",
      black: "#000000",
      bg: token("bg"),
      surface: {
        DEFAULT: token("surface"),
        2: token("surface-2"),
      },
      line: {
        DEFAULT: token("line"),
        soft: token("line-soft"),
      },
      fg: {
        DEFAULT: token("text"),
        2: token("text-2"),
        3: token("text-3"),
        4: token("text-4"),
      },
      accent: {
        ...tone("accent"),
        2: token("accent-2"),
        ink: token("accent-ink"),
        // shadcn: realce de item em menus/selects
        foreground: token("accent-ink"),
      },
      violet: { ...tone("violet"), brand: token("brand-violet") },
      ok: tone("ok"),
      warn: tone("warn"),
      danger: { ...tone("danger"), line: token("danger-line") },

      // Nomes do shadcn usados por components/ui (Radix), na mesma paleta.
      border: token("line"),
      input: token("line"),
      ring: token("accent"),
      background: token("bg"),
      foreground: token("text"),
      primary: { DEFAULT: token("accent"), foreground: token("accent-ink") },
      secondary: { DEFAULT: token("surface-2"), foreground: token("text") },
      destructive: { DEFAULT: token("danger-strong"), foreground: token("text") },
      muted: { DEFAULT: token("surface-2"), foreground: token("text-3") },
      popover: { DEFAULT: token("surface"), foreground: token("text") },
      card: { DEFAULT: token("surface"), foreground: token("text") },
    },
    extend: {
      fontFamily: {
        sans: ["var(--font-sans)"],
        mono: ["var(--font-mono)"],
      },
      borderRadius: {
        card: "var(--radius-card)",
        control: "var(--radius-control)",
        pill: "var(--radius-pill)",
        lg: "var(--radius-control)",
        md: "calc(var(--radius-control) - 2px)",
        sm: "calc(var(--radius-control) - 4px)",
      },
      transitionDuration: {
        fast: "120ms",
        base: "200ms",
      },
      transitionTimingFunction: {
        out: "cubic-bezier(0.2, 0.8, 0.2, 1)",
      },
      keyframes: {
        "accordion-down": {
          from: { height: "0" },
          to: { height: "var(--radix-accordion-content-height)" },
        },
        "accordion-up": {
          from: { height: "var(--radix-accordion-content-height)" },
          to: { height: "0" },
        },
      },
      animation: {
        "accordion-down": "accordion-down 0.2s ease-out",
        "accordion-up": "accordion-up 0.2s ease-out",
      },
    },
  },
  plugins: [tailwindcssAnimate],
} satisfies Config;
