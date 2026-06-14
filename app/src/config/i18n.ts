// CCResDoc i18n — single-locale (en only), no i18n.
// Minimal shim to satisfy imports from zudo-doc template patterns.

import { settings } from "./settings";

export const defaultLocale = settings.defaultLocale;
export type Locale = typeof defaultLocale;
export const locales = [defaultLocale] as const;

// Minimal translation helper — English only, returns the key's default value.
// Extend this map if multilingual support is added in a future Wave.
const translations = {
  "nav.overview": "Overview",
  "nav.backToMenu": "Main menu",
  "header.github": "GitHub",
} as const;

export type TranslationKey = keyof typeof translations;

const isDev =
  typeof import.meta !== "undefined" && (import.meta as { env?: { DEV?: boolean } }).env?.DEV === true;

export function t(key: TranslationKey | string, _locale?: string): string {
  const value = (translations as Record<string, string>)[key];
  if (isDev) {
    if (value === undefined) {
      console.warn(`[i18n] Missing translation key: "${key}"`);
    }
    if (_locale !== undefined && _locale !== defaultLocale) {
      console.warn(
        `[i18n] Locale "${_locale}" requested but only "${defaultLocale}" is supported`,
      );
    }
  }
  return value ?? key;
}
