import { actionPolicy, createTauriBackend, decodeBackendError } from "./settings-backend.mjs";

export const DEFAULT_DRAFT = Object.freeze({ schemaVersion: 1, claudeResources: true, codexResources: false, claudeDir: "~/.claude", codexDir: "~/.codex", appearanceMode: "system", themePack: "default", preferredPort: 4892, fallbackToFreePort: true, shortcuts: Object.freeze([]) });
export const FIELDS = Object.freeze(["claudeResources", "codexResources", "claudeDir", "codexDir", "appearanceMode", "themePack", "preferredPort", "fallbackToFreePort", "shortcuts"]);
const FIELD_IDS = { claudeResources: "claude-resources", codexResources: "codex-resources", claudeDir: "claude-dir", codexDir: "codex-dir", appearanceMode: "appearance-mode-group", themePack: "theme-pack", preferredPort: "preferred-port", fallbackToFreePort: "fallback-port" };
const DIAGNOSTIC_FIELDS = { "resources.claude": "claudeResources", "resources.codex": "codexResources", "source.claude_dir": "claudeDir", "source.codex_dir": "codexDir", "appearance.mode": "appearanceMode", "appearance.theme_pack": "themePack", "server.preferred_port": "preferredPort", "server.fallback_to_free_port": "fallbackToFreePort" };
export const cloneDraft = (draft) => ({ ...DEFAULT_DRAFT, ...(draft ?? {}), shortcuts: (draft?.shortcuts ?? DEFAULT_DRAFT.shortcuts).map((entry) => ({ commandId: entry.commandId, bindings: [...entry.bindings] })) });
const titleCase = (value) => String(value ?? "unknown").replaceAll("_", " ");
const groupLabel = (value) => titleCase(value).replace(/\b\w/g, (letter) => letter.toUpperCase());
const shortcutEntriesEqual = (left, right) => JSON.stringify(left ?? []) === JSON.stringify(right ?? []);
const MODIFIER_KEYS = new Set(["Alt", "AltGraph", "Control", "Meta", "Shift"]);
const KEY_NAMES = Object.freeze({ " ": "Space", Esc: "Escape", Left: "ArrowLeft", Right: "ArrowRight", Up: "ArrowUp", Down: "ArrowDown", Del: "Delete", Return: "Enter" });
const SHIFTED_PRINTABLE_KEYS = Object.freeze({ "!": "1", "@": "2", "#": "3", "$": "4", "%": "5", "^": "6", "&": "7", "*": "8", "(": "9", ")": "0", _: "-", "+": "=", "{": "[", "}": "]", "|": "\\", ":": ";", "\"": "'", "<": ",", ">": ".", "?": "/", "~": "`" });

export function portableBindingFromKeyEvent(event, { macos = false } = {}) {
  const rawKey = KEY_NAMES[event.key] ?? event.key;
  if (!rawKey || MODIFIER_KEYS.has(rawKey) || event.isComposing) return { kind: "modifier" };
  if (rawKey === "Escape") return { kind: "cancel" };
  if (event.getModifierState?.("AltGraph")) return { kind: "invalid", message: "AltGraph cannot be used as an app shortcut." };
  if (String(rawKey).length > 1 && String(rawKey).includes(" ") && rawKey !== "Space") return { kind: "invalid", message: "Shortcut chords are not supported. Press one modifier-and-key shortcut." };
  let key = event.shiftKey && SHIFTED_PRINTABLE_KEYS[rawKey] ? SHIFTED_PRINTABLE_KEYS[rawKey] : rawKey;
  key = key.length === 1 && /[a-z]/i.test(key) ? key.toUpperCase() : key;
  if (key === "Dead" || key === "Unidentified") return { kind: "invalid", message: "That key cannot be used as a shortcut." };
  const modifiers = [];
  if (macos ? event.metaKey : event.ctrlKey) modifiers.push("Mod");
  if (macos && event.ctrlKey) modifiers.push("Ctrl");
  if (event.altKey) modifiers.push("Alt");
  if (event.shiftKey) modifiers.push("Shift");
  if (modifiers.length === 0 && (key.length === 1 || key === "Space")) return { kind: "invalid", message: "Bare printable keys are not supported. Add Command or Control." };
  return { kind: "binding", binding: [...modifiers, key].join("+") };
}

export function displayShortcutBinding(binding, { macos = false } = {}) {
  const labels = macos ? { Mod: "⌘", Ctrl: "⌃", Alt: "⌥", Shift: "⇧" } : { Mod: "Control", Ctrl: "Control", Alt: "Alt", Shift: "Shift" };
  const parts = String(binding).split("+");
  return macos ? parts.map((part) => labels[part] ?? part).join("") : parts.map((part) => labels[part] ?? part).join("+");
}

export function applyDirectorySelection(input, selected) {
  if (selected === null) return false;
  input.value = selected;
  return true;
}

export function resourcePathDisabled({ busy, readOnly, enabled }) {
  return busy || readOnly || !enabled;
}

export function applyBundledAppearance(root, appearance) {
  if (!appearance || !["system", "light", "dark"].includes(appearance.mode)) return;
  const effective = appearance.mode === "system" ? (root.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light") : appearance.mode;
  root.document.documentElement.dataset.theme = effective;
  root.document.documentElement.style.colorScheme = effective;
  root.document.documentElement.dataset.themePack = appearance.themePack;
}

export function createSettingsEditor({ document, window, backend }) {
  const el = (id) => document.getElementById(id);
  const macos = /Mac|iPhone|iPad|iPod/i.test(window.navigator?.userAgentData?.platform ?? window.navigator?.platform ?? "");
  const appearanceRoot = { document, matchMedia: window.matchMedia?.bind(window) ?? globalThis.matchMedia.bind(globalThis) };
  const state = { snapshot: null, baseline: null, draft: cloneDraft(DEFAULT_DRAFT), displayAppearance: { mode: "system", themePack: "default" }, dirty: new Set(), validation: null, validationToken: 0, previewKey: "", busy: false, conflict: false, conflictCanRebase: null, phase: "loading", capture: null, captureToken: 0, captureSuspended: false };
  const draftControls = [...document.querySelectorAll('input[name="appearance-mode"], #theme-pack, #claude-resources, #codex-resources, #claude-dir, #codex-dir, #preferred-port, #fallback-port')];
  const actionControls = [el("pick-claude-source"), el("pick-codex-source"), el("reset-defaults"), el("reset-all-shortcuts"), el("reload-draft"), el("open-config"), el("reveal-config")];
  function showAppearance(value) { state.displayAppearance = { ...value }; applyBundledAppearance(appearanceRoot, value); }

  function setVisible(target) { for (const section of [el("settings-loading"), el("settings-form"), el("settings-fatal")]) section.hidden = section !== target; }
  function announce(text, { error = false, focus = false } = {}) { const message = el("global-message"); message.textContent = text; message.hidden = !text; message.classList.toggle("error", error); if (focus && text) message.focus(); }
  function setPhase(phase) { state.phase = phase; el("operation-status").textContent = ({ loading: "Loading…", validating: "Validating…", saving: "Saving…", applying: "Applying…", active: "Saved and active", saved_not_active: "Saved, not active" })[phase] ?? ""; }
  function readDraft() { const selectedMode = document.querySelector('input[name="appearance-mode"]:checked'); return { schemaVersion: state.baseline?.schemaVersion ?? 1, claudeResources: el("claude-resources").checked, codexResources: el("codex-resources").checked, claudeDir: el("claude-dir").value, codexDir: el("codex-dir").value, appearanceMode: selectedMode?.value ?? "", themePack: el("theme-pack").value, preferredPort: Number(el("preferred-port").value), fallbackToFreePort: el("fallback-port").checked, shortcuts: state.draft.shortcuts.map((entry) => ({ commandId: entry.commandId, bindings: [...entry.bindings] })) }; }
  function renderThemeOptions() { const select = el("theme-pack"); const supported = state.snapshot?.themePacks ?? ["default"]; const authored = state.draft.themePack; select.replaceChildren(); if (!supported.includes(authored)) { const unavailable = document.createElement("option"); unavailable.value = authored; unavailable.textContent = `${authored} (unavailable)`; select.append(unavailable); } for (const slug of supported) { const option = document.createElement("option"); option.value = slug; option.textContent = slug === "default" ? "Default" : slug; select.append(option); } select.value = authored; }
  function writeDraft(draft) { state.draft = cloneDraft(draft); for (const radio of document.querySelectorAll('input[name="appearance-mode"]')) radio.checked = radio.value === state.draft.appearanceMode; el("claude-resources").checked = state.draft.claudeResources; el("codex-resources").checked = state.draft.codexResources; el("claude-dir").value = state.draft.claudeDir; el("codex-dir").value = state.draft.codexDir; el("preferred-port").value = String(state.draft.preferredPort); el("fallback-port").checked = state.draft.fallbackToFreePort; renderThemeOptions(); renderShortcuts(); }
  function recomputeDirty() { state.draft = readDraft(); state.dirty = new Set(FIELDS.filter((field) => field === "shortcuts" ? !shortcutEntriesEqual(state.baseline?.shortcuts, state.draft.shortcuts) : state.baseline?.[field] !== state.draft[field])); }
  function diagnostics() { return state.validation?.diagnostics ?? state.snapshot?.settings?.validation ?? []; }
  function shortcutEntry(commandId) { return state.draft.shortcuts.find((entry) => entry.commandId === commandId); }
  function setShortcutBindings(commandId, bindings) {
    const entry = shortcutEntry(commandId);
    if (entry) entry.bindings = [...bindings];
    else state.draft.shortcuts.push({ commandId, bindings: [...bindings] });
  }
  function draftWithDefaultsPreservingUnknown() {
    const known = new Set((state.snapshot.shortcutCatalog?.commands ?? []).map((command) => command.commandId));
    const next = cloneDraft(state.snapshot.defaults);
    next.shortcuts.push(...state.draft.shortcuts.filter((entry) => !known.has(entry.commandId)).map((entry) => ({ commandId: entry.commandId, bindings: [...entry.bindings] })));
    return next;
  }
  function shortcutDiagnostics(commandId) { return diagnostics().filter((diagnostic) => diagnostic.blocking && diagnostic.field === `shortcuts.${commandId}`); }
  function focusShortcut(commandId) { document.querySelector(`[data-shortcut-command="${commandId}"] .shortcut-add`)?.focus(); }
  function renderShortcuts() {
    const root = el("shortcut-groups");
    if (!root || !state.snapshot) return;
    root.replaceChildren();
    const groups = new Map();
    for (const command of state.snapshot.shortcutCatalog?.commands ?? []) {
      if (!groups.has(command.group)) groups.set(command.group, []);
      groups.get(command.group).push(command);
    }
    const readOnly = state.snapshot.settings.status === "unsupported_version";
    for (const [group, commands] of groups) {
      const section = document.createElement("section");
      section.className = "shortcut-group";
      const heading = document.createElement("h3");
      heading.textContent = groupLabel(group);
      section.append(heading);
      const rows = document.createElement("div");
      rows.className = "shortcut-rows";
      for (const command of commands) {
        const row = document.createElement("div");
        row.className = "shortcut-row";
        row.dataset.shortcutCommand = command.commandId;
        const title = document.createElement("div");
        title.className = "shortcut-row-title";
        const label = document.createElement("strong");
        label.textContent = command.label;
        const reset = document.createElement("button");
        reset.type = "button";
        reset.className = "shortcut-reset quiet";
        reset.textContent = "Reset";
        reset.disabled = state.busy || readOnly || Boolean(state.capture);
        reset.addEventListener("click", async () => { setShortcutBindings(command.commandId, command.defaultBindings); renderShortcuts(); await changed(); focusShortcut(command.commandId); });
        title.append(label, reset);
        const controls = document.createElement("div");
        controls.className = "shortcut-controls";
        const bindings = shortcutEntry(command.commandId)?.bindings ?? command.defaultBindings;
        for (const binding of bindings) {
          const remove = document.createElement("button");
          remove.type = "button";
          remove.className = "shortcut-chip";
          remove.disabled = state.busy || readOnly || Boolean(state.capture);
          remove.setAttribute("aria-label", `Remove ${displayShortcutBinding(binding, { macos })} from ${command.label}`);
          const value = document.createElement("kbd");
          value.textContent = displayShortcutBinding(binding, { macos });
          const icon = document.createElement("span");
          icon.textContent = "×";
          icon.setAttribute("aria-hidden", "true");
          remove.append(value, icon);
          remove.addEventListener("click", async () => { setShortcutBindings(command.commandId, bindings.filter((candidate) => candidate !== binding)); renderShortcuts(); await changed(); focusShortcut(command.commandId); });
          controls.append(remove);
        }
        if (bindings.length === 0) {
          const empty = document.createElement("span");
          empty.className = "shortcut-empty";
          empty.textContent = "Unassigned";
          controls.append(empty);
        }
        const add = document.createElement("button");
        add.type = "button";
        add.className = "shortcut-add";
        const capturing = state.capture?.commandId === command.commandId;
        add.textContent = capturing ? (state.capture.ready ? "Press shortcut…" : "Preparing…") : "Add Binding";
        add.disabled = state.busy || readOnly || Boolean(state.capture && !capturing);
        add.setAttribute("aria-describedby", `shortcut-${command.commandId}-status shortcut-capture-help`);
        if (capturing) add.setAttribute("aria-pressed", "true");
        add.addEventListener("click", () => { if (!capturing) void beginCapture(command.commandId); });
        controls.append(add);
        const status = document.createElement("p");
        status.id = `shortcut-${command.commandId}-status`;
        status.className = "shortcut-status";
        status.setAttribute("aria-live", "polite");
        const errorMessages = shortcutDiagnostics(command.commandId).map((diagnostic) => diagnostic.message);
        const messages = capturing && state.capture.message ? [state.capture.message, ...errorMessages] : errorMessages;
        const hasError = errorMessages.length > 0 || Boolean(capturing && state.capture.error);
        status.textContent = messages.join(" ");
        status.hidden = messages.length === 0;
        status.classList.toggle("field-error", hasError);
        row.classList.toggle("has-error", hasError);
        add.setAttribute("aria-invalid", errorMessages.length ? "true" : "false");
        if (errorMessages.length) add.setAttribute("aria-errormessage", status.id);
        row.append(title, controls, status);
        rows.append(row);
      }
      section.append(rows);
      root.append(section);
    }
  }
  function renderErrors() { for (const [field, id] of Object.entries(FIELD_IDS)) { const control = el(id); const messages = diagnostics().filter((d) => DIAGNOSTIC_FIELDS[d.field] === field && d.blocking).map((d) => d.message); const error = el(`${field === "appearanceMode" ? "appearance-mode" : id}-error`); if (error) { error.textContent = messages.join(" "); error.hidden = messages.length === 0; } control?.setAttribute("aria-invalid", messages.length ? "true" : "false"); if (messages.length) control?.setAttribute("aria-errormessage", error?.id ?? ""); else control?.removeAttribute("aria-errormessage"); } }
  function renderStatus() { const settings = state.snapshot.settings; const runtime = state.snapshot.runtime; const active = runtime.active; const selection = (value) => value ? "Enabled" : "Disabled"; const path = (value, enabled, empty = "—") => enabled ? String(value ?? empty) : "Disabled"; el("config-path").textContent = settings.configPath; el("config-status").textContent = `${titleCase(settings.status)} · ${settings.fileExists ? "exists" : "not created"}`; el("config-revision").textContent = settings.revision ?? "None"; el("claude-selection-status").textContent = `${selection(settings.authored.claudeResources)} / ${selection(settings.effective?.claudeResources)} / ${active ? selection(active.claudeResources) : "not active"}`; el("claude-source-status").textContent = `${settings.authored.claudeDir} / ${path(settings.effective?.claudeDir, settings.effective?.claudeResources)} / ${active ? path(active.claudeDir, active.claudeResources, "not active") : "not active"}`; el("codex-selection-status").textContent = `${selection(settings.authored.codexResources)} / ${selection(settings.effective?.codexResources)} / ${active ? selection(active.codexResources) : "not active"}`; el("codex-source-status").textContent = `${settings.authored.codexDir} / ${path(settings.effective?.codexDir, settings.effective?.codexResources)} / ${active ? path(active.codexDir, active.codexResources, "not active") : "not active"}`; el("mode-status").textContent = `${settings.authored.appearanceMode} / ${active?.appearanceMode ?? "not active"}`; el("theme-status").textContent = `${settings.authored.themePack} / ${active?.themePack ?? "not active"}`; el("port-status").textContent = `${settings.authored.preferredPort} / ${active?.effectivePort ?? "not active"}`; el("runtime-status").textContent = titleCase(runtime.phase); el("fallback-status").textContent = runtime.fallbackUsed ? "Yes" : "No"; const list = el("diagnostics"); list.replaceChildren(); const all = [...settings.validation, ...(runtime.diagnostic ? [runtime.diagnostic] : [])]; for (const diagnostic of all) { const item = document.createElement("li"); const location = diagnostic.location ? ` (line ${diagnostic.location.line}, column ${diagnostic.location.column})` : ""; item.textContent = `${diagnostic.message}${location}`; list.append(item); } }
  function renderActions() { if (!state.snapshot) return; const policy = actionPolicy(state.snapshot); const readOnly = state.snapshot.settings.status === "unsupported_version"; for (const control of draftControls) control.disabled = state.busy || readOnly || Boolean(state.capture); for (const control of actionControls) control.disabled = state.busy || Boolean(state.capture); const claudePathDisabled = resourcePathDisabled({ busy: state.busy || Boolean(state.capture), readOnly, enabled: state.draft.claudeResources }); const codexPathDisabled = resourcePathDisabled({ busy: state.busy || Boolean(state.capture), readOnly, enabled: state.draft.codexResources }); el("claude-dir").disabled = claudePathDisabled; el("pick-claude-source").disabled = claudePathDisabled; el("codex-dir").disabled = codexPathDisabled; el("pick-codex-source").disabled = codexPathDisabled; el("reset-defaults").disabled ||= readOnly; el("reset-all-shortcuts").disabled ||= readOnly; el("save-settings").disabled = state.busy || Boolean(state.capture) || state.phase === "validating" || state.dirty.size === 0 || state.validation?.valid !== true || !policy.canSave || state.conflict; el("cancel-settings").disabled = state.busy; el("reapply").disabled = state.busy || Boolean(state.capture) || state.dirty.size === 0 || !policy.canRebase || state.conflictCanRebase === false; el("replace-malformed").disabled = state.busy || !policy.canReplaceMalformed; el("conflict-recovery").hidden = !state.conflict; el("malformed-recovery").hidden = state.snapshot.settings.status !== "malformed"; el("future-recovery").hidden = state.snapshot.settings.status !== "unsupported_version"; }
  function render() { renderStatus(); renderErrors(); renderShortcuts(); renderActions(); }

  async function endCapture({ message = "", focus = true } = {}) {
    const capture = state.capture;
    if (!capture && !state.captureSuspended) return;
    state.capture = null;
    state.captureToken += 1;
    try { await backend.setShortcutCaptureActive(false); }
    catch (error) { announce(`Could not restore shortcuts: ${decodeBackendError(error).message}`, { error: true, focus: true }); }
    state.captureSuspended = false;
    renderShortcuts();
    if (message) el("shortcut-live").textContent = message;
    if (focus && capture?.commandId) focusShortcut(capture.commandId);
  }
  async function beginCapture(commandId) {
    if (state.capture || state.busy) return;
    const token = ++state.captureToken;
    state.capture = { commandId, ready: false, error: false, message: "Suspending app shortcuts…", previousBindings: [...(shortcutEntry(commandId)?.bindings ?? [])] };
    renderShortcuts();
    focusShortcut(commandId);
    try {
      await backend.setShortcutCaptureActive(true);
      if (token !== state.captureToken || !state.capture) { await backend.setShortcutCaptureActive(false).catch(() => {}); return; }
      state.captureSuspended = true;
      state.capture.ready = true;
      state.capture.error = false;
      state.capture.message = "Press a modifier and one key, or Escape to cancel.";
      renderShortcuts();
      focusShortcut(commandId);
      el("shortcut-live").textContent = "Shortcut capture started.";
    } catch (error) {
      state.capture = null;
      await backend.setShortcutCaptureActive(false).catch(() => {});
      announce(`Shortcut capture could not start: ${decodeBackendError(error).message}`, { error: true, focus: true });
      renderShortcuts();
      focusShortcut(commandId);
    }
  }
  async function captureKeydown(event) {
    if (!state.capture) return false;
    event.preventDefault();
    event.stopPropagation();
    event.stopImmediatePropagation?.();
    if (!state.capture.ready) return true;
    const captured = portableBindingFromKeyEvent(event, { macos });
    if (captured.kind === "modifier") return true;
    if (captured.kind === "cancel") { await endCapture({ message: "Shortcut capture cancelled." }); return true; }
    if (captured.kind === "invalid") { state.capture.error = true; state.capture.message = captured.message; renderShortcuts(); focusShortcut(state.capture.commandId); el("shortcut-live").textContent = captured.message; return true; }
    const commandId = state.capture.commandId;
    const current = shortcutEntry(commandId)?.bindings ?? [];
    if (current.includes(captured.binding)) { state.capture.error = true; state.capture.message = `${displayShortcutBinding(captured.binding, { macos })} is already assigned to this action.`; renderShortcuts(); focusShortcut(commandId); el("shortcut-live").textContent = state.capture.message; return true; }
    setShortcutBindings(commandId, [...current, captured.binding]);
    await endCapture({ message: `${displayShortcutBinding(captured.binding, { macos })} added.` });
    await changed();
    focusShortcut(commandId);
    return true;
  }

  async function validate() { const token = ++state.validationToken; setPhase("validating"); renderActions(); try { const validation = await backend.validateDraft(state.draft); if (token !== state.validationToken) return; state.validation = validation; setPhase(""); render(); } catch (error) { if (token !== state.validationToken) return; setPhase(""); announce(decodeBackendError(error).message, { error: true }); renderActions(); } }
  async function previewAppearance() { const supported = state.snapshot?.themePacks ?? []; const value = { mode: state.draft.appearanceMode, themePack: supported.includes(state.draft.themePack) ? state.draft.themePack : state.snapshot?.settings?.effective?.themePack ?? "default" }; const key = `${value.mode}\n${value.themePack}`; if (key === state.previewKey || !["system", "light", "dark"].includes(value.mode)) return; state.previewKey = key; try { const envelope = await backend.previewAppearance(value.mode, value.themePack); showAppearance(envelope.appearance); } catch (error) { state.previewKey = ""; announce(`Appearance preview failed: ${decodeBackendError(error).message}`, { error: true }); } }
  async function acceptSnapshot(snapshot, { focus = false } = {}) { state.snapshot = snapshot; state.baseline = cloneDraft(snapshot.settings.authored); state.conflict = false; state.conflictCanRebase = null; state.dirty.clear(); state.validation = null; writeDraft(state.baseline); const appearance = { mode: snapshot.settings.effective.appearanceMode, themePack: snapshot.settings.effective.themePack }; state.previewKey = `${appearance.mode}\n${appearance.themePack}`; showAppearance(appearance); setVisible(el("settings-form")); setPhase(""); announce(""); render(); await validate(); if (focus) document.querySelector('input[name="appearance-mode"]:checked')?.focus(); }
  async function load({ focus = false, detectConflict = false, quiet = false } = {}) { if (!detectConflict && !quiet) { setVisible(el("settings-loading")); setPhase("loading"); } try { const snapshot = await backend.getSnapshot(); if (detectConflict && state.dirty.size && (snapshot.settings.revision ?? null) !== (state.snapshot?.settings?.revision ?? null)) { state.conflict = true; state.conflictCanRebase = snapshot.actions?.canRebase === true; announce("The config changed on disk while this draft was open.", { error: true, focus: true }); render(); return; } if (!detectConflict || state.dirty.size === 0) await acceptSnapshot(snapshot, { focus }); } catch (error) { if (detectConflict || quiet) { announce(`Could not refresh settings: ${decodeBackendError(error).message}`, { error: true }); return; } el("fatal-message").textContent = decodeBackendError(error).message; setVisible(el("settings-fatal")); setPhase(""); } }
  async function changed() { recomputeDirty(); if (!state.conflict) state.conflictCanRebase = null; announce(""); renderActions(); await Promise.all([validate(), previewAppearance()]); }
  async function performApply(operation) { await endCapture({ focus: false }); state.busy = true; setPhase("saving"); announce(""); renderActions(); await Promise.resolve(); setPhase("applying"); try { const result = await operation(); state.previewKey = ""; await load({ quiet: true }); setPhase(result.status === "saved_not_active" ? "saved_not_active" : "active"); if (result.status === "saved_not_active") announce("Settings were saved, but the runtime could not activate them. Review diagnostics and try again.", { error: true, focus: true }); } catch (error) { const decoded = decodeBackendError(error); if (decoded.code === "revision_conflict") state.conflict = true; try { const envelope = await backend.clearAppearancePreview(); showAppearance(envelope.appearance); } catch {} state.previewKey = ""; announce(decoded.code === "revision_conflict" ? "The config changed before Save completed. Reload or safely reapply your changed fields." : `${decoded.message} The appearance preview was rolled back.`, { error: true, focus: true }); setPhase(""); } finally { state.busy = false; render(); } }
  async function submit(event) { event?.preventDefault(); recomputeDirty(); if (el("save-settings").disabled) return; await performApply(() => backend.saveAndApply(state.draft, state.snapshot.settings.revision)); }
  async function reapply() { if (!window.confirm("Reapply only your changed fields onto the latest valid config? Comments and unknown keys will be preserved.")) return; await performApply(() => backend.rebaseStale(state.draft, state.dirty, state.snapshot.settings.revision)); }
  async function replaceMalformed() { if (!window.confirm("Replace the malformed config file with defaults? The current file contents will be overwritten.")) return; await performApply(() => backend.replaceMalformed(cloneDraft(state.snapshot.defaults), state.snapshot.settings.revision)); }
  async function reloadDraft() { if (state.dirty.size && !window.confirm("Reload settings from disk and discard this draft?")) return; await endCapture({ focus: false }); try { const envelope = await backend.clearAppearancePreview(); showAppearance(envelope.appearance); } catch {} state.previewKey = ""; await load({ focus: true }); }
  async function backendAction(action) { try { await action(); } catch (error) { announce(decodeBackendError(error).message, { error: true, focus: true }); } }
  function discardDraft() { writeDraft(state.baseline); state.dirty.clear(); state.conflict = false; state.conflictCanRebase = null; state.previewKey = ""; announce(""); render(); }
  async function cancel() {
    if (state.busy) return;
    await endCapture({ focus: false });
    discardDraft();
    try { const envelope = await backend.clearAppearancePreview(); showAppearance(envelope.appearance); } catch {}
    const currentWindow = window.__TAURI__?.window?.getCurrentWindow?.();
    if (typeof currentWindow?.hide === "function") await currentWindow.hide();
    else window.close();
  }

  document.querySelectorAll('input[name="appearance-mode"], #theme-pack, #claude-resources, #codex-resources, #claude-dir, #codex-dir, #preferred-port, #fallback-port').forEach((control) => control.addEventListener("change", changed)); el("claude-dir").addEventListener("input", changed); el("codex-dir").addEventListener("input", changed); el("preferred-port").addEventListener("input", changed);
  void backend.listenAppearance?.((envelope) => showAppearance(envelope.appearance));
  appearanceRoot.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => { if (state.displayAppearance.mode === "system") showAppearance(state.displayAppearance); });
  el("settings-form").addEventListener("submit", submit);
  for (const [buttonId, inputId] of [["pick-claude-source", "claude-dir"], ["pick-codex-source", "codex-dir"]]) el(buttonId).addEventListener("click", async () => { try { const input = el(inputId); const selected = await backend.pickSourceDirectory(); if (!applyDirectorySelection(input, selected)) return; await changed(); input.focus(); } catch (error) { announce(decodeBackendError(error).message, { error: true, focus: true }); } });
  el("reset-defaults").addEventListener("click", async () => { writeDraft(draftWithDefaultsPreservingUnknown()); await changed(); });
  el("reset-all-shortcuts").addEventListener("click", async () => { for (const command of state.snapshot.shortcutCatalog?.commands ?? []) setShortcutBindings(command.commandId, command.defaultBindings); renderShortcuts(); await changed(); el("shortcut-live").textContent = "All shortcuts reset to defaults in the draft."; });
  el("reload-draft").addEventListener("click", reloadDraft);
  el("reload-conflict").addEventListener("click", () => load({ focus: true }));
  el("reapply").addEventListener("click", reapply);
  el("replace-malformed").addEventListener("click", replaceMalformed);
  el("open-config").addEventListener("click", () => backendAction(() => backend.openConfigFile()));
  el("reveal-config").addEventListener("click", () => backendAction(() => backend.revealConfigFile()));
  el("cancel-settings").addEventListener("click", () => { void cancel(); });
  el("reload-settings").addEventListener("click", () => load({ focus: true }));
  document.addEventListener("keydown", (event) => { if (state.capture) { void captureKeydown(event); return; } if (event.key === "Escape" && !state.busy) { event.preventDefault(); void cancel(); } }, true); window.addEventListener("focus", () => { if (state.snapshot && !state.busy && !state.capture) load({ detectConflict: true }); }); window.addEventListener("blur", () => { void endCapture({ focus: false, message: "Shortcut capture cancelled when Settings lost focus." }); }); window.addEventListener("ccresdoc-settings-native-close", () => { void endCapture({ focus: false }); discardDraft(); void backend.clearAppearancePreview().catch(() => {}); }); window.addEventListener("pagehide", () => { void endCapture({ focus: false }); }); window.addEventListener("beforeunload", () => { void endCapture({ focus: false }); }); window.addEventListener("error", () => { void endCapture({ focus: false }); });
  return Object.freeze({ state, load, submit, reapply, replaceMalformed, cancel, beginCapture, endCapture, captureKeydown });
}

if (typeof document !== "undefined" && document.getElementById("settings-form")) { const editor = createSettingsEditor({ document, window, backend: createTauriBackend() }); editor.load({ focus: true }); }
