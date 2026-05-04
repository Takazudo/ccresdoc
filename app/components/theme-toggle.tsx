"use client";

import { useEffect, useState } from "preact/hooks";

type Theme = "light" | "dark";

// localStorage key — matches the key used in the pre-paint inline script.
const STORAGE_KEY = "ccresdoc.theme";

function readPersistedTheme(): Theme | null {
  try {
    const saved = window.localStorage.getItem(STORAGE_KEY);
    if (saved === "light" || saved === "dark") {
      return saved;
    }
  } catch {
    // Private browsing / disabled storage: fall through.
  }
  return null;
}

function readSystemTheme(): Theme {
  if (typeof window.matchMedia !== "function") return "dark";
  // Resolve order: prefer light only if explicitly set; default is dark.
  return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

/**
 * Theme toggle island.
 *
 * SSR contract: the first render (both server and client hydration) must be
 * deterministic. We render a neutral default here and sync to the real
 * preference in useEffect. The pre-paint inline script in layouts/default.tsx
 * has already applied the correct data-theme to <html>, so the page colours
 * are correct on first paint regardless of what this button renders.
 */
export default function ThemeToggle() {
  const [theme, setTheme] = useState<Theme>("dark");

  useEffect(() => {
    const initial = readPersistedTheme() ?? readSystemTheme();
    setTheme(initial);
  }, []);

  useEffect(() => {
    document.documentElement.dataset["theme"] = theme;
    try {
      window.localStorage.setItem(STORAGE_KEY, theme);
    } catch {
      // Storage unavailable; persistence is best-effort.
    }
  }, [theme]);

  const isDark = theme === "dark";
  const next: Theme = isDark ? "light" : "dark";

  return (
    <button
      type="button"
      class="theme-toggle"
      aria-pressed={isDark}
      aria-label={`Switch to ${next} theme`}
      onClick={() => setTheme(next)}
    >
      {isDark ? "☀ Light" : "☾ Dark"}
    </button>
  );
}
