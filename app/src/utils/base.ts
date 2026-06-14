// URL utilities (mirrors zudo-doc's base.ts, simplified for single-locale CCResDoc).

import { settings } from "@/config/settings";
import { defaultLocale } from "@/config/i18n";

const normalizedBase = settings.base.replace(/\/+$/, "");

// Asset extensions that should NOT receive a trailing slash.
// Restricted to a known allowlist so that dotted path segments like
// "project-v1.2" or "claude.md" (as a segment label) are not mistaken
// for files (fix #68).
const ASSET_EXT_RE =
  /\.(html|css|js|json|png|jpg|jpeg|svg|gif|webp|ico|txt|xml|map|woff2?)$/i;

export function applyTrailingSlash(url: string): string {
  if (!settings.trailingSlash) return url;
  if (url.endsWith("/")) return url;
  const suffixIdx = url.search(/[?#]/);
  const pathPart = suffixIdx >= 0 ? url.slice(0, suffixIdx) : url;
  const suffix = suffixIdx >= 0 ? url.slice(suffixIdx) : "";
  // Note: the `url.endsWith("/")` early-exit above already handles the
  // common path-with-trailing-slash case. This guard fires only when the
  // URL has a query/hash suffix and the path part itself ends with "/".
  if (pathPart.endsWith("/")) return url;
  const lastSegment = pathPart.split("/").pop() ?? "";
  if (ASSET_EXT_RE.test(lastSegment)) return url;
  return pathPart + "/" + suffix;
}

export function withBase(path: string): string {
  let raw: string;
  if (normalizedBase === "") {
    // When base is "/" it normalizes to "". Ensure the result is always an
    // absolute path so callers never get a bare relative string (fix #68).
    raw = path.startsWith("/") ? path : `/${path}`;
  } else {
    raw = `${normalizedBase}${path.startsWith("/") ? path : `/${path}`}`;
  }
  return applyTrailingSlash(raw);
}

export function stripBase(path: string): string {
  if (normalizedBase === "") return path;
  if (path === normalizedBase) return "/";
  return path.startsWith(`${normalizedBase}/`)
    ? path.slice(normalizedBase.length)
    : path;
}

// `lang` unused — CCResDoc is single-locale (EN only); locale-prefixed
// routes are not built. Parameter kept for API compatibility with
// multi-locale consumers.
export function docsUrl(slug: string, _lang: string = defaultLocale): string {
  // Trim leading slashes from slug to prevent double-slash paths like
  // "/docs//x" when slug already starts with "/" (fix #68).
  const trimmed = slug.replace(/^\/+/, "");
  const path = trimmed === "" ? "/docs" : `/docs/${trimmed}`;
  return withBase(path);
}

export function isExternal(href: string): boolean {
  return href.startsWith("http://") || href.startsWith("https://");
}

export function resolveHref(href: string): string {
  return isExternal(href) ? href : withBase(href);
}

export function navHref(path: string, _lang?: string, _version?: string): string {
  return withBase(path);
}
