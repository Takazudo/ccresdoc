import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { cloneDraft, DEFAULT_DRAFT, displayShortcutBinding, FIELDS, portableBindingFromKeyEvent } from "./settings-shell.mjs";

test("app-setting defaults match schema v3 while source catalog is managed separately", () => {
  assert.deepEqual(DEFAULT_DRAFT, {
    schemaVersion: 3,
    claudeResources: true,
    codexResources: false,
    claudeDir: "~/.claude",
    codexDir: "~/.codex",
    appearanceMode: "system",
    themePack: "classic",
    preferredPort: 4892,
    fallbackToFreePort: true,
    shortcuts: [],
  });
  assert.deepEqual(FIELDS.slice(0, 4), ["appearanceMode", "themePack", "preferredPort", "fallbackToFreePort"]);
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
  assert.deepEqual(portableBindingFromKeyEvent({ key: "k", ctrlKey: true, altKey: true, getModifierState: (name) => name === "AltGraph" }), { kind: "invalid", message: "AltGraph cannot be used as an app shortcut." });
  assert.equal(displayShortcutBinding("Mod+Shift+K", { macos: true }), "⌘⇧K");
  assert.equal(displayShortcutBinding("Mod+Shift+K"), "Control+Shift+K");
});

test("Settings links to the article and keeps bounded native recovery", async () => {
  const html = await readFile(new URL("./settings.html", import.meta.url), "utf8");
  assert.match(html, /id="source-summary"/);
  assert.match(html, /id="open-document-sources"/);
  assert.match(html, /id="source-recovery"/);
  assert.match(html, /id="recovery-disable"/);
  assert.match(html, /id="recovery-replace"/);
  assert.doesNotMatch(html, /id="claude-resources"|id="codex-resources"|id="claude-dir"|id="codex-dir"/);
  assert.match(html, /id="shortcut-groups"/);
  assert.match(html, /id="shortcut-live"[^>]+aria-live="polite"/);
});
