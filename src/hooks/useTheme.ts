import { useCallback, useEffect, useState } from "react";

/** "system" segue o tema do Windows (prefers-color-scheme) e muda junto com ele. */
export type Theme = "dark" | "light" | "system";
export type ResolvedTheme = "dark" | "light";

const KEY = "analystblaze.theme";
const QUERY = "(prefers-color-scheme: light)";

function systemTheme(): ResolvedTheme {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return "dark";
  return window.matchMedia(QUERY).matches ? "light" : "dark";
}

export function resolveTheme(theme: Theme): ResolvedTheme {
  return theme === "system" ? systemTheme() : theme;
}

function apply(theme: Theme) {
  const resolved = resolveTheme(theme);
  const root = document.documentElement;
  root.classList.toggle("light", resolved === "light");
  root.classList.toggle("dark", resolved === "dark");
  // design/tokens.css troca a paleta por este atributo.
  root.dataset.theme = resolved;
  root.style.colorScheme = resolved;
}

function read(): Theme {
  try {
    const value = localStorage.getItem(KEY);
    if (value === "light" || value === "dark" || value === "system") return value;
  } catch {
    // Fall through to the default theme.
  }
  return "dark";
}

export function useTheme() {
  const [theme, setThemeState] = useState<Theme>(() => {
    if (typeof window === "undefined") return "dark";
    const initial = read();
    apply(initial);
    return initial;
  });

  useEffect(() => {
    apply(theme);
    try {
      localStorage.setItem(KEY, theme);
    } catch {
      // Best effort only.
    }
    if (theme !== "system" || typeof window.matchMedia !== "function") return;
    // Acompanha o Windows enquanto "Sistema" estiver escolhido.
    const media = window.matchMedia(QUERY);
    const onChange = () => apply("system");
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, [theme]);

  const setTheme = useCallback((nextTheme: Theme) => setThemeState(nextTheme), []);
  const toggle = useCallback(() => setThemeState((current) => (resolveTheme(current) === "dark" ? "light" : "dark")), []);

  return { theme, resolved: resolveTheme(theme), setTheme, toggle };
}
