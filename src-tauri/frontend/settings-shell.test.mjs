import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { applyDirectorySelection, DEFAULT_DRAFT, FIELDS, resourcePathDisabled } from "./settings-shell.mjs";

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
  });
  assert.deepEqual(FIELDS.slice(0, 4), ["claudeResources", "codexResources", "claudeDir", "codexDir"]);
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
});
