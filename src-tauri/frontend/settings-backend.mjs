export const BACKEND_METHODS = Object.freeze([
  "retryLaunch",
  "openSettings",
  "getSnapshot",
  "validateDraft",
  "previewAppearance",
  "clearAppearancePreview",
  "listenAppearance",
  "saveAndApply",
  "rebaseStale",
  "replaceMalformed",
  "pickSourceDirectory",
  "openConfigFile",
  "revealConfigFile",
]);

/**
 * @typedef {{
 *   schemaVersion: number,
 *   claudeDir: string,
 *   appearanceMode: string,
 *   themePack: string,
 *   preferredPort: number,
 *   fallbackToFreePort: boolean
 * }} SettingsDraft
 * @typedef {"claudeDir"|"appearanceMode"|"themePack"|"preferredPort"|"fallbackToFreePort"} DirtyField
 * @typedef {{code: string, message: string, details: unknown}} BackendError
 */

export function decodeBackendError(error) {
  if (error && typeof error === "object" && typeof error.code === "string") {
    return {
      code: error.code,
      message: typeof error.message === "string" ? error.message : error.code,
      details: error.details ?? null,
    };
  }
  return {
    code: "unknown",
    message: error instanceof Error ? error.message : String(error ?? "Unknown backend error"),
    details: null,
  };
}

function mapKeys(value, rename) {
  if (Array.isArray(value)) return value.map((entry) => mapKeys(entry, rename));
  if (!value || typeof value !== "object") return value;
  return Object.fromEntries(Object.entries(value).map(([key, entry]) => [
    rename(key),
    mapKeys(entry, rename),
  ]));
}

const snakeToCamelKey = (key) => key.replace(/_([a-z])/g, (_, letter) => letter.toUpperCase());
const camelToSnakeKey = (key) => key.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`);
const fromWire = (value) => mapKeys(value, snakeToCamelKey);
const draftToWire = (draft) => mapKeys(draft, camelToSnakeKey);

export function createBackendAdapter(invoke, listen) {
  if (typeof invoke !== "function") throw new TypeError("invoke must be a function");
  const call = (command, args) => Promise.resolve(invoke(command, args)).catch((error) => {
    throw decodeBackendError(error);
  });
  return Object.freeze({
    retryLaunch: () => call("retry_launch"),
    openSettings: () => call("open_settings_window"),
    getSnapshot: () => call("get_settings_snapshot").then(fromWire),
    validateDraft: (draft) => call("validate_settings_draft", { draft: draftToWire(draft) }).then(fromWire),
    previewAppearance: (mode, themePack) => call("preview_appearance", { mode, themePack }).then(fromWire),
    clearAppearancePreview: () => call("clear_appearance_preview").then(fromWire),
    listenAppearance: (handler) => typeof listen === "function"
      ? Promise.resolve(listen("ccresdoc://appearance", (event) => handler(fromWire(event.payload))))
      : Promise.resolve(() => {}),
    saveAndApply: (draft, expectedRevision) => call("save_and_apply_settings", {
      draft: draftToWire(draft),
      expectedRevision: expectedRevision ?? null,
    }).then(fromWire),
    rebaseStale: (draft, dirtyFields, staleRevision) => call("rebase_stale_settings", {
      draft: draftToWire(draft),
      dirtyFields: [...dirtyFields].map(camelToSnakeKey),
      staleRevision,
    }).then(fromWire),
    replaceMalformed: (draft, expectedRevision) => call("replace_malformed_settings", {
      draft: draftToWire(draft),
      expectedRevision,
    }).then(fromWire),
    pickSourceDirectory: () => call("pick_source_directory"),
    openConfigFile: () => call("open_config_file"),
    revealConfigFile: () => call("reveal_config_file"),
  });
}

export function createTauriBackend(globalObject = globalThis) {
  const invoke = globalObject?.__TAURI__?.core?.invoke;
  if (typeof invoke !== "function") throw decodeBackendError("Tauri invoke API is unavailable");
  return createBackendAdapter(invoke, globalObject?.__TAURI__?.event?.listen);
}

export function actionPolicy(snapshot) {
  const status = snapshot?.settings?.status;
  const backend = snapshot?.actions ?? {};
  if (status === "unsupported_version") {
    return { canSave: false, canRebase: false, canReplaceMalformed: false };
  }
  if (status === "malformed") {
    return { canSave: false, canRebase: false, canReplaceMalformed: backend.canReplaceMalformed === true };
  }
  return {
    canSave: backend.canSave === true,
    canRebase: backend.canRebase === true,
    canReplaceMalformed: false,
  };
}
