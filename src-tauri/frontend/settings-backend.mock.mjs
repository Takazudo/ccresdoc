import { BACKEND_METHODS, actionPolicy } from "./settings-backend.mjs";

export function createMockBackend(overrides = {}) {
  const calls = [];
  const snapshot = overrides.snapshot ?? {
    settings: { status: "missing", revision: null, configPath: "/mock/config.toml" },
    runtime: { phase: "idle", active: null },
    actions: { canSave: true, canRebase: false, canReplaceMalformed: false },
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
