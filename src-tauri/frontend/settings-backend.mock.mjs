import { BACKEND_METHODS, actionPolicy } from "./settings-backend.mjs";

export function createMockBackend(overrides = {}) {
  const calls = [];
  const snapshot = overrides.snapshot ?? {
    settings: {
      status: "missing", revision: null, configPath: "/mock/config.toml", fileExists: false,
      authored: { schemaVersion: 1, claudeDir: "~/.claude", appearanceMode: "system", themePack: "default", preferredPort: 4892, fallbackToFreePort: true },
      effective: { claudeDir: "/mock/.claude", appearanceMode: "system", themePack: "default", preferredPort: 4892, effectivePort: 4892, fallbackToFreePort: true },
      validation: [],
    },
    runtime: { phase: "idle", active: null },
    actions: { canSave: true, canRebase: false, canReplaceMalformed: false },
    defaults: { schemaVersion: 1, claudeDir: "~/.claude", appearanceMode: "system", themePack: "default", preferredPort: 4892, fallbackToFreePort: true },
    themePacks: ["default"],
  };
  const result = (method, value) => (...args) => {
    calls.push({ method, args });
    return Promise.resolve(value);
  };
  const adapter = {
    retryLaunch: result("retryLaunch"),
    openSettings: result("openSettings"),
    getSnapshot: result("getSnapshot", snapshot),
    validateDraft: result("validateDraft", { valid: true, diagnostics: [] }),
    previewAppearance: result("previewAppearance", { appearance: { mode: "system", themePack: "default" } }),
    clearAppearancePreview: result("clearAppearancePreview", { appearance: { mode: "system", themePack: "default" } }),
    listenAppearance: result("listenAppearance", () => {}),
    saveAndApply: result("saveAndApply", { status: "saved_no_restart" }),
    rebaseStale: result("rebaseStale", { status: "saved_no_restart" }),
    replaceMalformed: result("replaceMalformed", { status: "saved_not_active" }),
    pickSourceDirectory: result("pickSourceDirectory", overrides.selection ?? null),
    openConfigFile: result("openConfigFile"),
    revealConfigFile: result("revealConfigFile"),
  };
  for (const method of BACKEND_METHODS) {
    if (typeof overrides[method] === "function") adapter[method] = overrides[method];
  }
  return { adapter: Object.freeze(adapter), calls, actionPolicy };
}
