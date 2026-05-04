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
  return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

function SunIcon() {
  return (
    <svg
      aria-hidden="true"
      xmlns="http://www.w3.org/2000/svg"
      width="20"
      height="20"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <circle cx="12" cy="12" r="5" />
      <line x1="12" y1="1" x2="12" y2="3" />
      <line x1="12" y1="21" x2="12" y2="23" />
      <line x1="4.22" y1="4.22" x2="5.64" y2="5.64" />
      <line x1="18.36" y1="18.36" x2="19.78" y2="19.78" />
      <line x1="1" y1="12" x2="3" y2="12" />
      <line x1="21" y1="12" x2="23" y2="12" />
      <line x1="4.22" y1="19.78" x2="5.64" y2="18.36" />
      <line x1="18.36" y1="5.64" x2="19.78" y2="4.22" />
    </svg>
  );
}

function MoonIcon() {
  return (
    <svg
      aria-hidden="true"
      xmlns="http://www.w3.org/2000/svg"
      width="20"
      height="20"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
    </svg>
  );
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
    document.documentElement.style.colorScheme = theme;
    try {
      window.localStorage.setItem(STORAGE_KEY, theme);
    } catch {
      // Storage unavailable; persistence is best-effort.
    }
    window.dispatchEvent(new CustomEvent("color-scheme-changed"));
  }, [theme]);

  const isDark = theme === "dark";
  const next: Theme = isDark ? "light" : "dark";

  return (
    <button
      type="button"
      class="text-muted hover:text-fg transition-colors p-hsp-sm focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-2"
      aria-label={`Switch to ${next} theme`}
      onClick={() => setTheme(next)}
    >
      {isDark ? <SunIcon /> : <MoonIcon />}
    </button>
  );
}
