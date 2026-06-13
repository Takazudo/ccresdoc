// CCResDoc i18n — single-locale (en only), no i18n.
// Minimal shim to satisfy imports from zudo-doc template patterns.

import { settings } from "./settings";

export const defaultLocale = settings.defaultLocale;
export type Locale = typeof defaultLocale;
export const locales = [defaultLocale] as const;

// Minimal translation helper — English only, returns the key's default value.
// Extend this map if multilingual support is added in a future Wave.
const translations: Record<string, string> = {
  "nav.overview": "Overview",
  "nav.backToMenu": "Main menu",
  "header.github": "GitHub",
};

export function t(key: string, _locale?: string): string {
  return translations[key] ?? key;
}
