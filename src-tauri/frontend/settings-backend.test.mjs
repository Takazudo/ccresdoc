import assert from "node:assert/strict";
import test from "node:test";
import { BACKEND_METHODS, actionPolicy, createBackendAdapter, decodeBackendError } from "./settings-backend.mjs";
import { createMockBackend } from "./settings-backend.mock.mjs";

test("adapter uses exact commands and camelCase arguments", async () => {
  const calls = [];
  const backend = createBackendAdapter((command, args) => { calls.push({ command, args }); return { ok: true }; });
  const draft = { schemaVersion: 1 };
  await backend.validateDraft(draft);
  await backend.previewAppearance("dark", "default");
  await backend.clearAppearancePreview();
  await backend.saveAndApply(draft, "sha256:one");
  await backend.rebaseStale(draft, new Set(["claudeDir"]), "sha256:stale");
  await backend.replaceMalformed(draft, "sha256:bad");
  assert.deepEqual(calls, [
    { command: "validate_settings_draft", args: { draft: { schema_version: 1 } } },
    { command: "preview_appearance", args: { mode: "dark", themePack: "default" } },
    { command: "clear_appearance_preview", args: undefined },
    { command: "save_and_apply_settings", args: { draft: { schema_version: 1 }, expectedRevision: "sha256:one" } },
    { command: "rebase_stale_settings", args: { draft: { schema_version: 1 }, dirtyFields: ["claude_dir"], staleRevision: "sha256:stale" } },
    { command: "replace_malformed_settings", args: { draft: { schema_version: 1 }, expectedRevision: "sha256:bad" } },
  ]);
});

test("adapter normalizes nested Rust snapshots to camelCase", async () => {
  const backend = createBackendAdapter(() => ({
    settings: { config_path: "/config", authored: { fallback_to_free_port: true } },
    runtime: { fallback_used: false, active: { effective_port: 53003 } },
  }));
  assert.deepEqual(await backend.getSnapshot(), {
    settings: { configPath: "/config", authored: { fallbackToFreePort: true } },
    runtime: { fallbackUsed: false, active: { effectivePort: 53003 } },
  });
});

test("adapter preserves structured backend errors", async () => {
  const backend = createBackendAdapter(() => Promise.reject({ code: "revision_conflict", message: "stale", details: { actualRevision: "new" } }));
  await assert.rejects(backend.getSnapshot(), { code: "revision_conflict", message: "stale", details: { actualRevision: "new" } });
  assert.equal(decodeBackendError(new Error("boom")).code, "unknown");
});

test("directory selection distinguishes selected and cancelled", async () => {
  const selected = createBackendAdapter(() => "/safe/source");
  assert.equal(await selected.pickSourceDirectory(), "/safe/source");
  const cancelled = createBackendAdapter(() => null);
  assert.equal(await cancelled.pickSourceDirectory(), null);
});

test("future schema and malformed policy cannot expose overwrite", () => {
  assert.deepEqual(actionPolicy({ settings: { status: "unsupported_version" }, actions: { canSave: true, canReplaceMalformed: true } }), {
    canSave: false, canRebase: false, canReplaceMalformed: false,
  });
  assert.deepEqual(actionPolicy({ settings: { status: "malformed" }, actions: { canReplaceMalformed: true } }), {
    canSave: false, canRebase: false, canReplaceMalformed: true,
  });
});

test("mock has exact adapter parity and records calls", async () => {
  const mock = createMockBackend({ selection: null });
  assert.deepEqual(Object.keys(mock.adapter).sort(), [...BACKEND_METHODS].sort());
  await mock.adapter.pickSourceDirectory();
  assert.deepEqual(mock.calls, [{ method: "pickSourceDirectory", args: [] }]);
});
