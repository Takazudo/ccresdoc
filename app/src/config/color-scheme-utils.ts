// Color scheme CSS generator for @takazudo/zudo-doc/theme ColorSchemeProvider.
//
// Builds the cssText prop: a `<style>` string with
// html[data-theme="light"] { --zd-* ... } and
// html[data-theme="dark"]  { --zd-* ... } blocks.

import { colorSchemes } from "./color-schemes";
import type { ColorScheme } from "./color-schemes";

function resolveColor(palette: ColorScheme["palette"], ref: number | string): string {
  if (typeof ref === "string") return ref;
  const value = palette[ref];
  if (value === undefined) throw new Error(`Palette index ${ref} out of range`);
  return value;
}

function schemeToCssPairs(scheme: ColorScheme): string {
  const p = scheme.palette;
  const sem = scheme.semantic ?? {};
  const resolve = (ref: number | string | undefined, fallback: number) =>
    resolveColor(p, ref ?? fallback);

  const lines: string[] = [
    `  --zd-bg: ${resolveColor(p, scheme.background)};`,
    `  --zd-fg: ${resolveColor(p, scheme.foreground)};`,
    `  --zd-cursor: ${resolveColor(p, scheme.cursor)};`,
    `  --zd-sel-bg: ${resolveColor(p, scheme.selectionBg)};`,
    `  --zd-sel-fg: ${resolveColor(p, scheme.selectionFg)};`,
  ];

  for (let i = 0; i < 16; i++) {
    lines.push(`  --zd-${i}: ${resolveColor(p, i)};`);
  }

  lines.push(
    `  --zd-surface: ${resolve(sem.surface, 10)};`,
    `  --zd-muted: ${resolve(sem.muted, 8)};`,
    `  --zd-accent: ${resolve(sem.accent, 5)};`,
    `  --zd-accent-hover: ${resolve(sem.accentHover, 14)};`,
    `  --zd-code-bg: ${resolve(sem.codeBg, 10)};`,
    `  --zd-code-fg: ${resolve(sem.codeFg, 11)};`,
    `  --zd-success: ${resolve(sem.success, 2)};`,
    `  --zd-danger: ${resolve(sem.danger, 1)};`,
    `  --zd-warning: ${resolve(sem.warning, 3)};`,
    `  --zd-info: ${resolve(sem.info, 4)};`,
    // Image overlay
    `  --zd-image-overlay-bg: ${resolve(sem.imageOverlayBg, 11)};`,
    `  --zd-image-overlay-fg: ${resolve(sem.imageOverlayFg, 10)};`,
    // Search highlight
    `  --zd-matched-keyword-bg: ${sem.matchedKeywordBg ?? "#fff59d"};`,
    `  --zd-matched-keyword-fg: ${sem.matchedKeywordFg ?? "#000000"};`,
    // Mermaid defaults (use bg/fg)
    `  --zd-mermaid-node-bg: ${resolveColor(p, scheme.background)};`,
    `  --zd-mermaid-text: ${resolveColor(p, scheme.foreground)};`,
    `  --zd-mermaid-line: ${resolve(sem.muted, 8)};`,
    `  --zd-mermaid-label-bg: ${resolve(sem.surface, 10)};`,
    // Derive from surface (like label-bg) so the note panel tracks the active
    // scheme. The old hardwired slot 0 rendered a dark box on the light bg;
    // sourcing from surface fixes the light scheme (dark surface is already
    // slot 0, so dark mode is unchanged).
    `  --zd-mermaid-note-bg: ${resolve(sem.surface, 10)};`,
  );

  return lines.join("\n");
}

/**
 * Build the CSS text for ColorSchemeProvider from the configured schemes.
 * Returns a string with one `html[data-theme="X"] { ... }` block per theme.
 */
export function buildColorSchemeCssText(themeToScheme: Record<string, string>): string {
  const blocks: string[] = [];
  for (const [themeKey, schemeName] of Object.entries(themeToScheme)) {
    const scheme = colorSchemes[schemeName];
    if (!scheme) throw new Error(`Color scheme "${schemeName}" not found`);
    blocks.push(`html[data-theme="${themeKey}"] {\n${schemeToCssPairs(scheme)}\n}`);
  }
  return blocks.join("\n\n");
}

/** Pre-built CSS text for the default CCResDoc light/dark themes. */
export const colorSchemeCssText = buildColorSchemeCssText({
  light: "Default Light",
  dark: "Default Dark",
});
