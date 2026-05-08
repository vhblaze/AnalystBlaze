import { useCallback, useEffect, useState } from "react";

export type Theme = "dark" | "light";

const KEY = "analystblaze.theme";

function apply(theme: Theme) {
  const root = document.documentElement;
  root.classList.toggle("light", theme === "light");
  root.classList.toggle("dark", theme === "dark");
  root.style.colorScheme = theme;
}

function read(): Theme {
  try {
    const value = localStorage.getItem(KEY);
    if (value === "light" || value === "dark") return value;
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
  }, [theme]);

  const setTheme = useCallback((nextTheme: Theme) => setThemeState(nextTheme), []);
  const toggle = useCallback(() => setThemeState((current) => (current === "dark" ? "light" : "dark")), []);

  return { theme, setTheme, toggle };
}
