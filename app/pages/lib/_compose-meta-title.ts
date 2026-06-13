// Page meta-title composer for CCResDoc.
//
// Format: "<page title> | CCResDoc"
// Root index: "CCResDoc"

import { settings } from "@/config/settings";

export function composeMetaTitle(pageTitle?: string): string {
  if (!pageTitle) return settings.siteName;
  return `${pageTitle} | ${settings.siteName}`;
}
