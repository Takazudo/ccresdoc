import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act } from "preact/test-utils";
import { render } from "preact";
import { beforeEach, describe, expect, it, vi } from "vitest";
// The bundled editor is deliberately framework-free and shared verbatim with Tauri.
import { createSettingsEditor, DEFAULT_DRAFT } from "../../src-tauri/frontend/settings-shell.mjs";
import { openSettingsFromDocs, SettingsHeaderButton } from "../pages/lib/_chrome";
import shortcutCatalog from "../src/browser-chrome/command-catalog.json";

const htmlPath = resolve(process.cwd(), "../src-tauri/frontend/settings.html");
const shellHtml = readFileSync(htmlPath, "utf8").match(/<main[\s\S]*<\/main>/)?.[0] ?? "";
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
const defaultShortcuts = () => shortcutCatalog.commands.map(({ commandId, defaultBindings }) => ({ commandId, bindings: [...defaultBindings] }));

function snapshot(overrides: Record<string, unknown> = {}) {
  const base = {
    settings: {
      configPath: "/Users/test/.config/ccresdoc/config.toml", fileExists: true, status: "valid", revision: "sha256:one",
      authored: { ...DEFAULT_DRAFT, shortcuts: defaultShortcuts() }, effective: { ...DEFAULT_DRAFT, shortcuts: defaultShortcuts(), claudeDir: "/Users/test/.claude", effectivePort: 4892 },
      active: { usesAuthoredSettings: true, sourceIsAuthored: true, preferredPort: 4892, effectivePort: 4892 }, validation: [],
    },
    runtime: { phase: "ready", active: { ...DEFAULT_DRAFT, shortcuts: defaultShortcuts(), claudeDir: "/Users/test/.claude", effectivePort: 4892 }, fallbackUsed: false, diagnostic: null },
    actions: { canSave: true, canRebase: true, canReplaceMalformed: false }, defaults: { ...DEFAULT_DRAFT, shortcuts: defaultShortcuts() }, themePacks: ["default", "paper"], shortcutCatalog,
  };
  return { ...base, ...overrides } as any;
}

function setup(initial = snapshot(), withNativeHide = false) {
  document.body.innerHTML = shellHtml;
  let current = initial;
  const calls: Array<[string, ...unknown[]]> = [];
  const backend = {
    getSnapshot: vi.fn(async () => current),
    validateDraft: vi.fn(async (draft) => ({ valid: Number(draft.preferredPort) >= 1 && Number(draft.preferredPort) <= 65535, effective: draft, diagnostics: Number(draft.preferredPort) >= 1 && Number(draft.preferredPort) <= 65535 ? [] : [{ kind: "invalid_port", field: "server.preferred_port", message: "port must be in 1..=65535", blocking: true }] })),
    previewAppearance: vi.fn(async (mode, themePack) => ({ appearance: { mode, themePack } })),
    clearAppearancePreview: vi.fn(async () => ({ appearance: { mode: current.settings.effective.appearanceMode, themePack: current.settings.effective.themePack } })),
    listenAppearance: vi.fn(async () => () => {}),
    saveAndApply: vi.fn(async (draft, revision) => { calls.push(["save", draft, revision]); current = snapshot({ settings: { ...current.settings, authored: { ...draft }, revision: "sha256:two" } }); return { status: "active" }; }),
    rebaseStale: vi.fn(async (...args) => { calls.push(["rebase", ...args]); return { status: "active" }; }),
    replaceMalformed: vi.fn(async (...args) => { calls.push(["replace", ...args]); current = snapshot(); return { status: "saved_not_active" }; }),
    pickSourceDirectory: vi.fn(async (): Promise<string | null> => null), openConfigFile: vi.fn(async () => {}), revealConfigFile: vi.fn(async () => {}),
    setShortcutCaptureActive: vi.fn(async () => {}),
  };
  const close = vi.fn();
  const hide = vi.fn(async () => {});
  const editorWindow = { addEventListener: window.addEventListener.bind(window), close, confirm: vi.fn(() => true), ...(withNativeHide ? { __TAURI__: { window: { getCurrentWindow: () => ({ hide }) } } } : {}) } as any;
  const editor = createSettingsEditor({ document, window: editorWindow, backend });
  return { editor, backend, calls, close, hide, setSnapshot(value: any) { current = value; } };
}

describe("bundled Settings editor", () => {
  beforeEach(() => { vi.restoreAllMocks(); });

  it("loads the complete draft, exposes semantic labels/descriptions, and focuses the first choice", async () => {
    const { editor } = setup(); await editor.load({ focus: true }); await flush();
    expect((document.querySelector('[name="appearance-mode"]:checked') as HTMLInputElement).value).toBe("system");
    expect(document.activeElement).toBe(document.querySelector('[name="appearance-mode"]:checked'));
    expect(document.querySelector('label[for="claude-dir"]')).not.toBeNull();
    expect(document.querySelector("#claude-dir")?.getAttribute("aria-describedby")).toContain("claude-dir-description");
    expect(document.querySelector("#claude-source-status")?.textContent).toBe("~/.claude / /Users/test/.claude / /Users/test/.claude");
    expect(document.querySelector("#port-status")?.textContent).toBe("4892 / 4892");
    expect((document.querySelector("#save-settings") as HTMLButtonElement).disabled).toBe(true);
    expect([...document.querySelectorAll(".shortcut-group h3")].map((node) => node.textContent)).toEqual(["Navigation", "Discovery", "Page"]);
    expect(document.querySelectorAll(".shortcut-row")).toHaveLength(shortcutCatalog.commands.length);
  });

  it("captures only after native accelerators suspend, preserves unknown entries, and saves shortcut arrays without an early snapshot mutation", async () => {
    const authored = [...defaultShortcuts(), { commandId: "future-command-v2", bindings: ["future+syntax"] }];
    const current = setup(snapshot({ settings: { ...snapshot().settings, authored: { ...DEFAULT_DRAFT, shortcuts: authored } } }));
    let releaseSuspend!: () => void;
    current.backend.setShortcutCaptureActive.mockImplementationOnce(() => new Promise<void>((resolve) => { releaseSuspend = resolve; }));
    await current.editor.load(); await flush();
    const backRow = document.querySelector('[data-shortcut-command="back"]')!;
    (backRow.querySelector(".shortcut-add") as HTMLButtonElement).click();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "b", ctrlKey: true, bubbles: true, cancelable: true }));
    expect(current.editor.state.draft.shortcuts.find((entry: any) => entry.commandId === "back").bindings).toEqual(["Mod+["]);
    releaseSuspend(); await flush();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Control", ctrlKey: true, bubbles: true, cancelable: true }));
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "b", ctrlKey: true, bubbles: true, cancelable: true }));
    await flush(); await flush();
    expect(current.backend.setShortcutCaptureActive.mock.calls).toEqual([[true], [false]]);
    expect((current.editor.state.snapshot as any).settings.authored.shortcuts.find((entry: any) => entry.commandId === "back").bindings).toEqual(["Mod+["]);
    expect(current.editor.state.draft.shortcuts.find((entry: any) => entry.commandId === "back").bindings).toEqual(["Mod+[", "Mod+B"]);
    expect(current.editor.state.draft.shortcuts.find((entry: any) => entry.commandId === "future-command-v2").bindings).toEqual(["future+syntax"]);
    await current.editor.submit(); await flush();
    const saved = current.backend.saveAndApply.mock.calls[0][0];
    expect(saved.shortcuts.find((entry: any) => entry.commandId === "future-command-v2").bindings).toEqual(["future+syntax"]);
    (document.querySelector("#reset-defaults") as HTMLButtonElement).click(); await flush();
    expect(current.editor.state.draft.shortcuts.find((entry: any) => entry.commandId === "future-command-v2").bindings).toEqual(["future+syntax"]);
  });

  it("keeps capture focus on duplicate/conflict feedback and Escape cancels before ambient close", async () => {
    const current = setup();
    current.backend.validateDraft.mockImplementation(async (draft) => {
      const reserved = draft.shortcuts.find((entry: any) => entry.commandId === "home")?.bindings.includes("Mod+Q");
      return { valid: !reserved, effective: draft, diagnostics: reserved ? [{ kind: "shortcut_conflict", field: "shortcuts.home", message: "Home conflicts with reserved action Quit (Mod+Q)", blocking: true }] : [] };
    });
    await current.editor.load(); await flush();
    const homeAdd = document.querySelector('[data-shortcut-command="home"] .shortcut-add') as HTMLButtonElement;
    homeAdd.click(); await flush();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "q", ctrlKey: true, bubbles: true, cancelable: true })); await flush(); await flush();
    expect(document.querySelector('[data-shortcut-command="home"] .shortcut-status')?.textContent).toContain("Home conflicts with reserved action Quit");
    expect(document.querySelector('[data-shortcut-command="home"] .shortcut-add')?.getAttribute("aria-errormessage")).toBe("shortcut-home-status");
    expect(document.activeElement).toBe(document.querySelector('[data-shortcut-command="home"] .shortcut-add'));
    expect((document.querySelector("#save-settings") as HTMLButtonElement).disabled).toBe(true);

    const forwardBefore = [...current.editor.state.draft.shortcuts.find((entry: any) => entry.commandId === "forward").bindings];
    (document.querySelector('[data-shortcut-command="forward"] .shortcut-add') as HTMLButtonElement).click(); await flush();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true })); await flush();
    expect(current.close).not.toHaveBeenCalled();
    expect(current.editor.state.draft.shortcuts.find((entry: any) => entry.commandId === "forward").bindings).toEqual(forwardBefore);
    expect(current.backend.setShortcutCaptureActive.mock.calls.slice(-2)).toEqual([[true], [false]]);
  });

  it("supports chip removal, command/all reset, and suspension cleanup on native close", async () => {
    const current = setup(); await current.editor.load(); await flush();
    const backRow = document.querySelector('[data-shortcut-command="back"]')!;
    (backRow.querySelector(".shortcut-chip") as HTMLButtonElement).click(); await flush();
    expect(current.editor.state.draft.shortcuts.find((entry: any) => entry.commandId === "back").bindings).toEqual([]);
    (document.querySelector('[data-shortcut-command="back"] .shortcut-reset') as HTMLButtonElement).click(); await flush();
    expect(current.editor.state.draft.shortcuts.find((entry: any) => entry.commandId === "back").bindings).toEqual(["Mod+["]);
    (document.querySelector('[data-shortcut-command="back"] .shortcut-chip') as HTMLButtonElement).click(); await flush();
    (document.querySelector("#reset-all-shortcuts") as HTMLButtonElement).click(); await flush();
    expect(current.editor.state.draft.shortcuts.find((entry: any) => entry.commandId === "back").bindings).toEqual(["Mod+["]);
    (document.querySelector('[data-shortcut-command="back"] .shortcut-add') as HTMLButtonElement).click(); await flush();
    window.dispatchEvent(new Event("ccresdoc-settings-native-close")); await flush();
    expect(current.backend.setShortcutCaptureActive.mock.calls.slice(-2)).toEqual([[true], [false]]);
    expect(current.editor.state.dirty.size).toBe(0);

    (document.querySelector('[data-shortcut-command="back"] .shortcut-add') as HTMLButtonElement).click(); await flush();
    window.dispatchEvent(new Event("blur")); await flush();
    expect(current.backend.setShortcutCaptureActive.mock.calls.slice(-2)).toEqual([[true], [false]]);
    expect(current.editor.state.capture).toBeNull();
    expect(document.querySelector("#shortcut-live")?.textContent).toContain("lost focus");
  });

  it("keeps authored and effective ports distinct when the runtime falls back", async () => {
    const fallback = setup(snapshot({
      settings: {
        ...snapshot().settings,
        authored: { ...DEFAULT_DRAFT, preferredPort: 4892 },
        effective: { ...snapshot().settings.effective, preferredPort: 4892, effectivePort: 4892 },
      },
      runtime: {
        phase: "ready",
        active: { ...DEFAULT_DRAFT, claudeDir: "/tmp/isolated-source", preferredPort: 4892, effectivePort: 53001 },
        fallbackUsed: true,
        diagnostic: null,
      },
    }));
    await fallback.editor.load(); await flush();
    expect(document.querySelector("#port-status")?.textContent).toBe("4892 / 53001");
    expect(document.querySelector("#fallback-status")?.textContent).toBe("Yes");
    expect(document.querySelector("#runtime-status")?.textContent).toBe("ready");
  });

  it("tracks dirty validation, associates errors, and awaits save/apply without closing", async () => {
    const { editor, backend, close } = setup(); await editor.load(); await flush();
    const port = document.querySelector("#preferred-port") as HTMLInputElement; port.value = "0"; port.dispatchEvent(new Event("input", { bubbles: true })); await flush();
    expect(port.getAttribute("aria-invalid")).toBe("true"); expect(port.getAttribute("aria-errormessage")).toBe("preferred-port-error");
    expect((document.querySelector("#save-settings") as HTMLButtonElement).disabled).toBe(true); expect(backend.saveAndApply).not.toHaveBeenCalled();
    port.value = "5000"; port.dispatchEvent(new Event("input", { bubbles: true })); await flush();
    expect((document.querySelector("#save-settings") as HTMLButtonElement).disabled).toBe(false);
    await editor.submit(); await flush(); expect(backend.saveAndApply).toHaveBeenCalledWith(expect.objectContaining({ preferredPort: 5000 }), "sha256:one"); expect(close).not.toHaveBeenCalled(); expect(document.querySelector("#operation-status")?.textContent).toBe("Saved and active");
  });

  it("keeps the editor visible and disables mutation throughout an async apply", async () => {
    const { editor, backend, setSnapshot } = setup(); await editor.load();
    const source = document.querySelector("#claude-dir") as HTMLInputElement; source.value = "/pending"; source.dispatchEvent(new Event("input", { bubbles: true })); await flush();
    let finish!: (value: { status: string }) => void;
    backend.saveAndApply.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    const applying = editor.submit(); await flush();
    expect(document.querySelector("#settings-form")?.hasAttribute("hidden")).toBe(false); expect(source.disabled).toBe(true); expect(document.querySelector("#operation-status")?.textContent).toBe("Applying…");
    setSnapshot(snapshot({ settings: { ...snapshot().settings, authored: { ...DEFAULT_DRAFT, claudeDir: "/pending" }, revision: "sha256:two" } })); finish({ status: "active" }); await applying;
    expect(source.disabled).toBe(false); expect(document.querySelector("#operation-status")?.textContent).toBe("Saved and active");
  });

  it("treats picker cancellation as a no-op, selected paths as drafts, Reset as unsaved, and Escape as Cancel", async () => {
    const { editor, backend, close } = setup(); await editor.load(); await flush();
    const source = document.querySelector("#claude-dir") as HTMLInputElement;
    (document.querySelector("#pick-claude-source") as HTMLButtonElement).click(); await flush(); expect(source.value).toBe("~/.claude");
    backend.pickSourceDirectory.mockResolvedValueOnce("/safe/resources"); (document.querySelector("#pick-claude-source") as HTMLButtonElement).click(); await flush(); expect(source.value).toBe("/safe/resources"); expect(backend.saveAndApply).not.toHaveBeenCalled();
    (document.querySelector("#reset-defaults") as HTMLButtonElement).click(); await flush(); expect(source.value).toBe("~/.claude"); expect(backend.saveAndApply).not.toHaveBeenCalled();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true })); await flush(); expect(close).toHaveBeenCalledOnce();
  });

  it("keeps failed saves open and distinguishes saved-not-active", async () => {
    const failed = setup(); await failed.editor.load(); await flush();
    failed.backend.saveAndApply.mockRejectedValueOnce({ code: "io", message: "disk full" });
    const source = document.querySelector("#claude-dir") as HTMLInputElement; source.value = "/changed"; source.dispatchEvent(new Event("input", { bubbles: true })); await flush(); await failed.editor.submit();
    expect(document.querySelector("#global-message")?.textContent).toContain("disk full"); expect(document.querySelector("#global-message")?.textContent).toContain("rolled back"); expect(failed.backend.clearAppearancePreview).toHaveBeenCalled(); expect(failed.close).not.toHaveBeenCalled(); expect(source.value).toBe("/changed");

    const saved = setup(); await saved.editor.load(); await flush(); saved.backend.saveAndApply.mockImplementationOnce(async (draft) => { saved.setSnapshot(snapshot({ runtime: { phase: "saved_not_active", active: null, fallbackUsed: false, diagnostic: { kind: "spawn_failed", message: "could not restart" } }, settings: { ...snapshot().settings, authored: draft } })); return { status: "saved_not_active" }; });
    const changed = document.querySelector("#claude-dir") as HTMLInputElement; changed.value = "/new"; changed.dispatchEvent(new Event("input", { bubbles: true })); await flush(); await saved.editor.submit();
    expect(document.querySelector("#operation-status")?.textContent).toBe("Saved, not active"); expect(document.querySelector("#global-message")?.textContent).toContain("could not activate");
  });

  it("previews appearance live and Cancel resolves the backend's latest authority", async () => {
    const current = setup(snapshot(), true); await current.editor.load(); await flush();
    const dark = document.querySelector('[name="appearance-mode"][value="dark"]') as HTMLInputElement;
    dark.checked = true; dark.dispatchEvent(new Event("change", { bubbles: true })); await flush();
    expect(current.backend.previewAppearance).toHaveBeenCalledWith("dark", "default");
    expect(document.documentElement.dataset.theme).toBe("dark");
    current.backend.clearAppearancePreview.mockResolvedValueOnce({ appearance: { mode: "light", themePack: "default" } });
    await current.editor.cancel(); await flush();
    expect(current.backend.clearAppearancePreview).toHaveBeenCalled();
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(current.hide).toHaveBeenCalledOnce();
    expect(current.close).not.toHaveBeenCalled();
    expect((document.querySelector("#save-settings") as HTMLButtonElement).disabled).toBe(true);
  });

  it("enforces malformed replacement, future read-only, unavailable theme, and stale rebase recovery", async () => {
    const malformed = snapshot({ settings: { ...snapshot().settings, status: "malformed", validation: [{ kind: "malformed_syntax", field: null, message: "expected value", blocking: true, location: { line: 4, column: 2 } }] }, actions: { canSave: false, canRebase: false, canReplaceMalformed: true } });
    const recovery = setup(malformed); await recovery.editor.load(); await flush(); expect((document.querySelector("#save-settings") as HTMLButtonElement).disabled).toBe(true); expect(document.querySelector("#diagnostics")?.textContent).toContain("line 4, column 2"); await recovery.editor.replaceMalformed(); expect(recovery.backend.replaceMalformed).toHaveBeenCalledWith(snapshot().defaults, "sha256:one");

    const future = setup(snapshot({ settings: { ...snapshot().settings, status: "unsupported_version" }, actions: { canSave: false, canRebase: false, canReplaceMalformed: false } })); await future.editor.load(); await flush(); expect((document.querySelector("#claude-dir") as HTMLInputElement).disabled).toBe(true); expect((document.querySelector("#replace-malformed") as HTMLButtonElement).disabled).toBe(true);

    const unavailable = setup(snapshot({ settings: { ...snapshot().settings, authored: { ...DEFAULT_DRAFT, themePack: "gone" }, effective: { ...snapshot().settings.effective, themePack: "default" }, validation: [{ kind: "theme_pack_unavailable", field: "appearance.theme_pack", message: "gone unavailable", blocking: false }] } })); await unavailable.editor.load(); await flush(); expect((document.querySelector("#theme-pack") as HTMLSelectElement).value).toBe("gone"); expect(document.querySelector("#theme-status")?.textContent).toContain("gone / default"); const dark = document.querySelector('[name="appearance-mode"][value="dark"]') as HTMLInputElement; dark.checked = true; dark.dispatchEvent(new Event("change", { bubbles: true })); await flush(); expect(unavailable.backend.previewAppearance).toHaveBeenCalledWith("dark", "default");

    const stale = setup(); await stale.editor.load(); await flush(); const port = document.querySelector("#preferred-port") as HTMLInputElement; port.value = "5001"; port.dispatchEvent(new Event("input", { bubbles: true })); await flush(); stale.setSnapshot(snapshot({ settings: { ...snapshot().settings, revision: "sha256:external" } })); await stale.editor.load({ detectConflict: true }); expect(document.querySelector("#conflict-recovery")?.hasAttribute("hidden")).toBe(false); port.value = "5002"; port.dispatchEvent(new Event("input", { bubbles: true })); await flush(); expect(document.querySelector("#conflict-recovery")?.hasAttribute("hidden")).toBe(false); stale.backend.rebaseStale.mockRejectedValueOnce({ code: "revision_conflict", message: "changed again" }); await stale.editor.reapply(); expect(document.querySelector("#conflict-recovery")?.hasAttribute("hidden")).toBe(false); expect(stale.backend.rebaseStale).toHaveBeenCalledWith(expect.objectContaining({ preferredPort: 5002 }), new Set(["preferredPort"]), "sha256:one");
  });
});

describe("documentation header Settings entry point", () => {
  it("renders Settings unavailable outside Tauri", () => {
    delete (globalThis as any).__TAURI__;
    const mount = document.createElement("div"); document.body.append(mount);
    act(() => render(SettingsHeaderButton(), mount));
    const button = mount.querySelector("button")!;
    expect(button.disabled).toBe(true);
    expect(button.getAttribute("aria-disabled")).toBe("true");
  });

  it("renders an accessible gear and invokes only the narrow open command", async () => {
    const invoke = vi.fn(async () => {}); const root = { __TAURI__: { core: { invoke } } } as any;
    await openSettingsFromDocs(root); expect(invoke).toHaveBeenCalledWith("open_settings_window");
    const mount = document.createElement("div"); document.body.append(mount); (globalThis as any).__TAURI__ = root.__TAURI__;
    act(() => render(SettingsHeaderButton(), mount)); const button = mount.querySelector("button")!; expect(button.getAttribute("aria-label")).toBe("Open Settings"); expect(button.title).toBe("Settings"); expect(button.disabled).toBe(false); button.click(); await flush(); expect(invoke).toHaveBeenCalledTimes(2); delete (globalThis as any).__TAURI__;
  });
});
