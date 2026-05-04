/**
 * Color scheme definitions ported from $HOME/.claude/doc/src/config/color-schemes.ts.
 * Each scheme's palette maps p0–p15 to --zd-0 through --zd-15 CSS custom properties.
 * Semantic overrides and base variables (bg, fg, sel-bg, sel-fg) are resolved from
 * palette indices at build time and injected by ColorSchemeProvider.
 */

export interface ColorScheme {
  background: number;
  foreground: number;
  cursor: number;
  selectionBg: number;
  selectionFg: number;
  palette: [
    string, string, string, string, string, string, string, string,
    string, string, string, string, string, string, string, string,
  ];
  semantic?: {
    surface?: number;
    muted?: number;
    accent?: number;
    accentHover?: number;
    codeBg?: number;
    codeFg?: number;
    success?: number;
    danger?: number;
    warning?: number;
    info?: number;
    mermaidNodeBg?: number;
    mermaidText?: number;
    mermaidLine?: number;
    mermaidLabelBg?: number;
    mermaidNoteBg?: number;
    chatUserBg?: number;
    chatUserText?: number;
    chatAssistantBg?: number;
    chatAssistantText?: number;
  };
}

/**
 * Semantic defaults used when scheme.semantic does not override a key.
 * Indices correspond to the palette array (p0–p15).
 */
export const SEMANTIC_DEFAULTS = {
  surface: 9,
  muted: 8,
  accent: 5,
  accentHover: 14,
  codeBg: 10,
  codeFg: 11,
  success: 2,
  danger: 1,
  warning: 3,
  info: 4,
  mermaidNodeBg: 9,
  mermaidText: 11,
  mermaidLine: 8,
  mermaidLabelBg: 10,
  mermaidNoteBg: 0,
  chatUserBg: 5,
  chatUserText: 9,
  chatAssistantBg: 9,
  chatAssistantText: 11,
} as const;

export const colorSchemes: Record<string, ColorScheme> = {
  "Default Light": {
    background: 9,
    foreground: 11,
    cursor: 6,
    selectionBg: 11,
    selectionFg: 10,
    palette: [
      "#303030", "#dd3131", "#266538", "#a83838",
      "#3277c8", "#a35e0f", "#90a1b9", "#7a5218",
      "#6b6b6b", "#e2ddda", "#ece9e9", "#303030",
      "#5b99dc", "#b89ee7", "#8590a0", "#654516",
    ],
    semantic: {
      surface: 10,
      muted: 8,
      accent: 5,
      accentHover: 14,
      codeBg: 10,
      codeFg: 11,
      success: 2,
      danger: 1,
      warning: 3,
      info: 4,
    },
  },
  "Default Dark": {
    background: 9,
    foreground: 15,
    cursor: 6,
    selectionBg: 10,
    selectionFg: 11,
    palette: [
      "#1c1c1c", "#da6871", "#93bb77", "#dfbb77",
      "#5caae9", "#c074d6", "#90a1b9", "#a0a0a0",
      "#888888", "#181818", "#383838", "#e0e0e0",
      "#d69a66", "#c074d6", "#a7c0e3", "#b8b8b8",
    ],
    semantic: {
      surface: 0,
      muted: 8,
      accent: 12,
      accentHover: 14,
      codeBg: 10,
      codeFg: 11,
      success: 2,
      danger: 1,
      warning: 3,
      info: 4,
    },
  },
};

/** Maps data-theme attribute values to scheme names. */
export const THEME_TO_SCHEME: Record<string, string> = {
  light: "Default Light",
  dark: "Default Dark",
};

/** The default theme applied when no preference is detected. */
export const DEFAULT_THEME = "dark";
