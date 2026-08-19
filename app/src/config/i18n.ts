// Temporary single-locale adapter for host-owned routes. Issue #96 replaces
// it with package route context; translation data is already package-owned.

import { defaultTranslations } from "@takazudo/zudo-doc/i18n-defaults";
import { settings } from "./settings";

export const defaultLocale = settings.defaultLocale;
export type Locale = typeof defaultLocale;
export const locales = [defaultLocale] as const;

const translations = defaultTranslations.en as Record<string, string>;
export type TranslationKey = keyof typeof defaultTranslations.en;

const isDev =
  typeof import.meta !== "undefined" && (import.meta as { env?: { DEV?: boolean } }).env?.DEV === true;

export function t(key: TranslationKey | string, _locale?: string): string {
  const value = translations[key];
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
