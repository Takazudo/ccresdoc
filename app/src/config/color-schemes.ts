// CCResDoc color schemes — default light and dark schemes.
// Mirrors the standard zudo-doc palette convention.

export interface ColorScheme {
  background: number | string;
  foreground: number | string;
  cursor: number | string;
  selectionBg: number | string;
  selectionFg: number | string;
  palette: [
    string, string, string, string, string, string, string, string,
    string, string, string, string, string, string, string, string,
  ];
  semantic?: {
    surface?: number | string;
    muted?: number | string;
    accent?: number | string;
    accentHover?: number | string;
    codeBg?: number | string;
    codeFg?: number | string;
    success?: number | string;
    danger?: number | string;
    warning?: number | string;
    info?: number | string;
    imageOverlayBg?: number | string;
    imageOverlayFg?: number | string;
    matchedKeywordBg?: string;
    matchedKeywordFg?: string;
  };
}

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
      imageOverlayBg: 11,
      imageOverlayFg: 10,
      matchedKeywordBg: "#fff59d",
      matchedKeywordFg: "#000000",
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
      imageOverlayBg: 0,
      imageOverlayFg: 11,
      matchedKeywordBg: "#fff59d",
      matchedKeywordFg: "#000000",
    },
  },
};
