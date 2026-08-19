// Browser-safe single-locale adapter retained only by the legacy navigation
// islands. Server routes use pages/lib/_route-context.ts directly.

import { defaultTranslations } from "@takazudo/zudo-doc/i18n-defaults";
import type { FactoryI18n } from "@takazudo/zudo-doc/factory-context";
import { makeUrlHelpers } from "@takazudo/zudo-doc/url-helpers";
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

/**
 * Browser-safe package contexts retained for the legacy navigation islands.
 * Issue #97 replaces those islands directly; keeping this adapter here avoids
 * making a server-side route context reachable from their client bundles.
 */
export const i18n = {
  defaultLocale,
  locales,
  getLocaleLabel: (locale: string) => locale.toUpperCase(),
  t,
} satisfies FactoryI18n;

export const urlHelpers = makeUrlHelpers(settings, i18n);
