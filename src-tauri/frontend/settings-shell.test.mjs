import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { applyDirectorySelection, cloneDraft, DEFAULT_DRAFT, displayShortcutBinding, FIELDS, portableBindingFromKeyEvent, resourcePathDisabled } from "./settings-shell.mjs";

test("resource defaults and dirty fields match the additive schema-v1 contract", () => {
  assert.deepEqual(DEFAULT_DRAFT, {
    schemaVersion: 1,
    claudeResources: true,
    codexResources: false,
    claudeDir: "~/.claude",
    codexDir: "~/.codex",
    appearanceMode: "system",
    themePack: "default",
    preferredPort: 4892,
    fallbackToFreePort: true,
    shortcuts: [],
  });
  assert.deepEqual(FIELDS.slice(0, 4), ["claudeResources", "codexResources", "claudeDir", "codexDir"]);
  assert.equal(FIELDS.at(-1), "shortcuts");
});

test("shortcut drafts deep-clone binding arrays and key events use neutral Mod storage", () => {
  const source = { ...DEFAULT_DRAFT, shortcuts: [{ commandId: "back", bindings: ["Mod+["] }] };
  const draft = cloneDraft(source);
  draft.shortcuts[0].bindings.push("Mod+B");
  assert.deepEqual(source.shortcuts[0].bindings, ["Mod+["]);
  assert.deepEqual(portableBindingFromKeyEvent({ key: "b", ctrlKey: true }), { kind: "binding", binding: "Mod+B" });
  assert.deepEqual(portableBindingFromKeyEvent({ key: "q" }), { kind: "invalid", message: "Bare printable keys are not supported. Add Command or Control." });
  assert.deepEqual(portableBindingFromKeyEvent({ key: "Shift", shiftKey: true }), { kind: "modifier" });
  assert.deepEqual(portableBindingFromKeyEvent({ key: "Escape" }), { kind: "cancel" });
  assert.deepEqual(portableBindingFromKeyEvent({ key: "+", ctrlKey: true, shiftKey: true }), { kind: "binding", binding: "Mod+Shift+=" });
  assert.equal(displayShortcutBinding("Mod+Shift+K", { macos: true }), "⌘⇧K");
  assert.equal(displayShortcutBinding("Mod+Shift+K"), "Control+Shift+K");
});

test("directory picker updates only its selected source and preserves cancellation", () => {
  const claude = { value: "~/.claude" };
  const codex = { value: "~/.codex" };
  assert.equal(applyDirectorySelection(codex, "/picked/codex"), true);
  assert.equal(codex.value, "/picked/codex");
  assert.equal(claude.value, "~/.claude");
  assert.equal(applyDirectorySelection(claude, null), false);
  assert.equal(claude.value, "~/.claude");
});

test("resource paths disable independently and all controls lock while applying or read-only", () => {
  assert.equal(resourcePathDisabled({ busy: false, readOnly: false, enabled: true }), false);
  assert.equal(resourcePathDisabled({ busy: false, readOnly: false, enabled: false }), true);
  assert.equal(resourcePathDisabled({ busy: true, readOnly: false, enabled: true }), true);
  assert.equal(resourcePathDisabled({ busy: false, readOnly: true, enabled: true }), true);
});

test("resource controls remain native, labelled, and independently addressable", async () => {
  const html = await readFile(new URL("./settings.html", import.meta.url), "utf8");
  for (const id of ["claude-resources", "codex-resources"]) {
    assert.match(html, new RegExp(`<input id="${id}" type="checkbox"`));
  }
  for (const id of ["claude-dir", "codex-dir"]) {
    assert.match(html, new RegExp(`<label for="${id}">`));
    assert.match(html, new RegExp(`<input id="${id}" type="text"`));
  }
  assert.match(html, /id="pick-claude-source"/);
  assert.match(html, /id="pick-codex-source"/);
  assert.match(html, /id="shortcut-groups"/);
  assert.match(html, /id="shortcut-live"[^>]+aria-live="polite"/);
});
