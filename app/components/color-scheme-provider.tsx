/**
 * ColorSchemeProvider — emits a static <style> tag with all --zd-* CSS
 * custom properties scoped to html[data-theme="light"] and html[data-theme="dark"].
 *
 * The THEME_BOOTSTRAP_SCRIPT in default.tsx sets data-theme on <html> before
 * paint, so browsers apply the correct variable block without any FOUC.
 * No client-side JS is required here — the style tag is fully static.
 *
 * Ported from $HOME/.claude/doc/src/components/color-scheme-provider.astro and
 * $HOME/.claude/doc/src/config/color-scheme-utils.ts (schemeToCssPairs logic).
 */

import {
  colorSchemes,
  SEMANTIC_DEFAULTS,
  THEME_TO_SCHEME,
  type ColorScheme,
} from "../config/color-schemes";

function resolve(palette: ColorScheme["palette"], index: number): string {
  const value = palette[index];
  if (value === undefined) {
    throw new Error(`Palette index ${index} out of range`);
  }
  return value;
}

function schemeToCss(scheme: ColorScheme): string {
  const p = scheme.palette;
  const sem = scheme.semantic ?? {};

  const lines: string[] = [
    `  --zd-bg: ${resolve(p, scheme.background)};`,
    `  --zd-fg: ${resolve(p, scheme.foreground)};`,
    `  --zd-cursor: ${resolve(p, scheme.cursor)};`,
    `  --zd-sel-bg: ${resolve(p, scheme.selectionBg)};`,
    `  --zd-sel-fg: ${resolve(p, scheme.selectionFg)};`,
  ];

  for (let i = 0; i < 16; i++) {
    lines.push(`  --zd-${i}: ${resolve(p, i)};`);
  }

  const get = (key: keyof typeof SEMANTIC_DEFAULTS): string => {
    const idx = (sem as Record<string, number | undefined>)[key] ?? SEMANTIC_DEFAULTS[key];
    return resolve(p, idx);
  };

  lines.push(
    `  --zd-surface: ${get("surface")};`,
    `  --zd-muted: ${get("muted")};`,
    `  --zd-accent: ${get("accent")};`,
    `  --zd-accent-hover: ${get("accentHover")};`,
    `  --zd-code-bg: ${get("codeBg")};`,
    `  --zd-code-fg: ${get("codeFg")};`,
    `  --zd-success: ${get("success")};`,
    `  --zd-danger: ${get("danger")};`,
    `  --zd-warning: ${get("warning")};`,
    `  --zd-info: ${get("info")};`,
    `  --zd-mermaid-node-bg: ${get("mermaidNodeBg")};`,
    `  --zd-mermaid-text: ${get("mermaidText")};`,
    `  --zd-mermaid-line: ${get("mermaidLine")};`,
    `  --zd-mermaid-label-bg: ${get("mermaidLabelBg")};`,
    `  --zd-mermaid-note-bg: ${get("mermaidNoteBg")};`,
    `  --zd-chat-user-bg: ${get("chatUserBg")};`,
    `  --zd-chat-user-text: ${get("chatUserText")};`,
    `  --zd-chat-assistant-bg: ${get("chatAssistantBg")};`,
    `  --zd-chat-assistant-text: ${get("chatAssistantText")};`,
  );

  return lines.join("\n");
}

function buildStyleSheet(): string {
  const blocks: string[] = [];

  for (const [themeKey, schemeName] of Object.entries(THEME_TO_SCHEME)) {
    const scheme = colorSchemes[schemeName];
    if (!scheme) {
      throw new Error(`Color scheme "${schemeName}" not found`);
    }
    blocks.push(
      `html[data-theme="${themeKey}"] {\n${schemeToCss(scheme)}\n}`,
    );
  }

  return blocks.join("\n\n");
}

const STYLE_CONTENT = buildStyleSheet();

export default function ColorSchemeProvider() {
  return (
    <style
      id="zd-color-scheme"
      dangerouslySetInnerHTML={{ __html: STYLE_CONTENT }}
    />
  );
}
