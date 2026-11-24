"use client";

import { getScope } from "@takazudo/zfb/zudo-react";
import {
  applyPresetRules, cloneCatalog, duplicateSource, localCatalogErrors,
  saveRulesAsPreset, sourceFromPreset,
  type Catalog, type Preset, type Source,
} from "./catalog";
import { installUnsavedNavigationGuard } from "./navigation-guard";

type Diagnostic = { field?: string | null; message: string; blocking?: boolean };
type EditorSnapshot = {
  catalog: Catalog; builtinPresets: Preset[]; revision: string | null;
  loadStatus: string; validation: Diagnostic[]; runtimePhase: string;
  runtimeGeneration: number; activeSourceIds: string[]; runtimeError: string | null;
  pendingApply: boolean;
};

const escape = (value: unknown) => String(value ?? "").replace(/[&<>"']/g, (char) => ({
  "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
})[char] ?? char);
const id = () => globalThis.crypto?.randomUUID?.() ?? `source-${Date.now()}-${Math.random().toString(36).slice(2)}`;
const same = (left: unknown, right: unknown) => JSON.stringify(left) === JSON.stringify(right);

function backend() {
  const invoke = (globalThis as typeof globalThis & { __TAURI__?: { core?: { invoke?: (name: string, args?: unknown) => Promise<unknown> } } }).__TAURI__?.core?.invoke;
  if (typeof invoke !== "function") throw new Error("The source editor is available in the CCResDoc app.");
  return {
    get: () => invoke("get_source_editor_snapshot") as Promise<EditorSnapshot>,
    validate: (catalog: Catalog, expectedGeneration: number) => invoke("validate_source_editor_draft", { catalog, expectedGeneration }) as Promise<Diagnostic[]>,
    save: (catalog: Catalog, expectedRevision: string | null, expectedGeneration: number) => invoke("save_source_editor_draft", { catalog, expectedRevision, expectedGeneration }) as Promise<EditorSnapshot>,
    rebase: (catalog: Catalog, baseCatalog: Catalog, staleRevision: string, expectedGeneration: number) => invoke("rebase_source_editor_draft", { catalog, baseCatalog, staleRevision, expectedGeneration }) as Promise<EditorSnapshot>,
    apply: (expectedRevision: string | null, expectedGeneration: number) => invoke("apply_saved_sources", { expectedRevision, expectedGeneration }) as Promise<{ status: string }>,
    pick: (expectedGeneration: number) => invoke("pick_document_source_directory", { expectedGeneration }) as Promise<string | null>,
  };
}

function errorMessage(error: unknown): string {
  if (error && typeof error === "object" && "message" in error) return String(error.message);
  return String(error ?? "Unknown error");
}

function lines(value: string): string[] {
  return value.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
}

function rulesSummary(rules: Preset["rules"]): string {
  if (rules.kind !== "custom") return rules.kind === "codex" ? "Specialized Codex renderer" : "Specialized Claude Code renderer";
  return `${rules.format}; include: ${rules.include.join(", ") || "none"}; exclude: ${rules.exclude.join(", ") || "none"}`;
}

function presetOptions(presets: Preset[], selected?: string) {
  return presets.map((preset) => `<option value="${escape(preset.id)}"${preset.id === selected ? " selected" : ""}>${escape(preset.name)}</option>`).join("");
}

function field(source: Source, key: string, label: string, content: string, help = "") {
  const fieldId = `source-${escape(source.id)}-${key}`;
  return `<div class="ccresdoc-source-manager__field"><label for="${fieldId}">${label}</label>${content}${help ? `<small>${help}</small>` : ""}<small role="alert" data-error-for="sources.${escape(source.id)}.${key}" hidden></small></div>`;
}

function sourceCard(source: Source, allPresets: Preset[], active: boolean) {
  const sourceId = escape(source.id);
  const customRules = source.rules.kind === "custom" ? source.rules : null;
  const scanAvailable = source.rules.kind === "claude_code" || source.rules.kind === "codex";
  const origin = source.origin_preset?.name ?? "Custom rules";
  const sourceField = (key: string, label: string, help = "") => field(source, key, label,
    `<input id="source-${sourceId}-${key}" data-source="${sourceId}" data-field="${key}" value="${escape(source[key as keyof Source])}"${key === "root" && !source.enabled ? " disabled" : ""} autocomplete="off" spellcheck="false">`, help);
  const rootField = field(source, "root", "Folder", `<div class="ccresdoc-source-manager__path"><input id="source-${sourceId}-root" data-source="${sourceId}" data-field="root" value="${escape(source.root)}"${!source.enabled ? " disabled" : ""} autocomplete="off" spellcheck="false"><button type="button" data-action="pick-root" data-source="${sourceId}"${!source.enabled ? " disabled" : ""}>Choose…</button></div>`, "Stored even when disabled. Original files are never changed.");
  const scanField = scanAvailable ? `<div class="ccresdoc-source-manager__field"><label for="source-${sourceId}-scan">Instructions scan</label><select id="source-${sourceId}-scan" data-source="${sourceId}" data-field="scan_kind"><option value="same"${source.scan_root.kind === "same" ? " selected" : ""}>Same folder</option><option value="separate"${source.scan_root.kind === "separate" ? " selected" : ""}>Separate folder</option></select></div>${source.scan_root.kind === "separate" ? field(source, "scan_root", "Instructions folder", `<div class="ccresdoc-source-manager__path"><input id="source-${sourceId}-scan_root" data-source="${sourceId}" data-field="scan_root" value="${escape(source.scan_root.path)}"${!source.enabled ? " disabled" : ""} autocomplete="off" spellcheck="false"><button type="button" data-action="pick-scan" data-source="${sourceId}"${!source.enabled ? " disabled" : ""}>Choose…</button></div>`) : ""}` : "";
  const customFields = customRules ? `<div class="ccresdoc-source-manager__fields">${field(source, "rules.format", "Read files as", `<select id="source-${sourceId}-rules.format" data-source="${sourceId}" data-field="rules.format"><option value="markdown"${customRules.format === "markdown" ? " selected" : ""}>Markdown</option><option value="code"${customRules.format === "code" ? " selected" : ""}>Code and text</option><option value="skill"${customRules.format === "skill" ? " selected" : ""}>Skill packages</option></select>`)}${field(source, "rules.include", "Include patterns", `<textarea id="source-${sourceId}-rules.include" data-source="${sourceId}" data-field="rules.include" spellcheck="false">${escape(customRules.include.join("\n"))}</textarea>`, "One glob pattern per line.")}${field(source, "rules.exclude", "Exclude patterns", `<textarea id="source-${sourceId}-rules.exclude" data-source="${sourceId}" data-field="rules.exclude" spellcheck="false">${escape(customRules.exclude.join("\n"))}</textarea>`, "Optional; one glob pattern per line.")}</div>` : `<p class="ccresdoc-source-manager__muted">This built-in preset uses its specialized ${source.rules.kind === "codex" ? "Codex" : "Claude Code"} renderer.</p>`;
  return `<section class="ccresdoc-source-manager__source" data-source-card="${sourceId}" data-disabled="${!source.enabled}">
    <div class="ccresdoc-source-manager__source-head"><label><input type="checkbox" data-source="${sourceId}" data-field="enabled"${source.enabled ? " checked" : ""}><strong>${escape(source.label || "Untitled source")}</strong></label><span>${active ? "Active" : source.enabled ? "Saved or draft" : "Disabled"}</span></div>
    <p class="ccresdoc-source-manager__muted">Preset: ${escape(origin)}. This source keeps an independent rule snapshot.</p>
    <div class="ccresdoc-source-manager__fields">${sourceField("label", "Source name")}${sourceField("mount", "URL name", "Changing this removes the old generated route on Apply.")}${rootField}${scanField}</div>
    <details class="ccresdoc-source-manager__rules" data-rules="${sourceId}"><summary>Reading rules</summary>${customFields}<div class="ccresdoc-source-manager__actions"><select aria-label="Preset rules to use" data-preset-choice="${sourceId}">${presetOptions(allPresets, source.origin_preset?.id)}</select><button type="button" data-action="use-preset" data-source="${sourceId}">Use preset rules…</button><button type="button" data-action="save-rules-preset" data-source="${sourceId}">Save rules as preset…</button></div></details>
    <div class="ccresdoc-source-manager__actions"><button type="button" data-action="duplicate" data-source="${sourceId}">Duplicate</button><button type="button" data-action="remove" data-source="${sourceId}" data-danger>Remove source</button></div>
  </section>`;
}

export function startDocumentSourcesManager(root: HTMLElement): () => void {
  const api = backend();
  let snapshot: EditorSnapshot | null = null;
  let baseCatalog: Catalog = { sources: [], presets: [] };
  let draft: Catalog = { sources: [], presets: [] };
  let diagnostics: Diagnostic[] = [];
  let dirty = false;
  let conflict = false;
  let busy = false;
  let message = "Loading document sources…";
  let error = false;
  let validateToken = 0;
  let stopped = false;
  const expanded = new Set<string>();
  const dialog = document.createElement("dialog");
  dialog.className = "ccresdoc-source-manager__dialog";
  root.after(dialog);
  const allPresets = () => [...(snapshot?.builtinPresets ?? []), ...draft.presets];
  const findSource = (sourceId: string) => draft.sources.find((source) => source.id === sourceId);
  const findPreset = (presetId: string) => allPresets().find((preset) => preset.id === presetId);
  const setMessage = (value: string, isError = false) => { message = value; error = isError; const node = root.querySelector<HTMLElement>("[data-manager-status]"); if (node) { node.textContent = value; node.dataset.error = String(isError); } };
  const updateDirty = () => { dirty = !same(baseCatalog, draft); updateActions(); };
  const updateActions = () => {
    root.querySelectorAll<HTMLButtonElement>("[data-action=save]").forEach((button) => { button.disabled = busy || !dirty || conflict; });
    root.querySelectorAll<HTMLButtonElement>("[data-action=apply]").forEach((button) => { button.disabled = busy || dirty || conflict || !snapshot || snapshot.loadStatus === "unsupported_version"; });
    root.querySelectorAll<HTMLButtonElement>("[data-action=discard]").forEach((button) => { button.disabled = busy || !dirty; });
  };
  const renderErrors = () => {
    root.querySelectorAll<HTMLElement>("[data-error-for]").forEach((node) => { const issue = diagnostics.find((entry) => entry.field === node.dataset.errorFor); node.textContent = issue?.message ?? ""; node.hidden = !issue; const input = node.parentElement?.querySelector<HTMLElement>("input,textarea,select"); input?.setAttribute("aria-invalid", issue ? "true" : "false"); });
  };
  const render = () => {
    const open = root.querySelectorAll<HTMLDetailsElement>("details[data-rules]");
    for (const item of open) if (item.open) expanded.add(item.dataset.rules ?? ""); else expanded.delete(item.dataset.rules ?? "");
    const active = new Set(snapshot?.activeSourceIds ?? []);
    const presets = allPresets();
    root.innerHTML = `<div class="ccresdoc-source-manager__notice" role="status" aria-live="polite" data-manager-status data-error="${error}">${escape(message)}</div>
      <section class="ccresdoc-source-manager__section" aria-labelledby="manager-sources-title"><div class="ccresdoc-source-manager__section-head"><h2 id="manager-sources-title">Sources</h2><span>${draft.sources.filter((source) => source.enabled).length} enabled of ${draft.sources.length}</span></div>
      <div class="ccresdoc-source-manager__actions"><label for="manager-add-preset">Start from preset</label><select id="manager-add-preset">${presetOptions(presets, "builtin-markdown")}</select><button type="button" data-action="add-source">Add source</button></div>
      ${draft.sources.length ? draft.sources.map((source) => sourceCard(source, presets, active.has(source.id))).join("") : "<p>No sources configured. Add a folder to browse its documents.</p>"}</section>
      <section class="ccresdoc-source-manager__section" aria-labelledby="manager-presets-title"><div class="ccresdoc-source-manager__section-head"><h2 id="manager-presets-title">Reusable presets</h2><button type="button" data-action="new-preset">New preset</button></div><p>Presets contain rules only. Editing or deleting one leaves existing source snapshots alone.</p><div class="ccresdoc-source-manager__preset-list">${presets.map((preset) => `<div class="ccresdoc-source-manager__preset"><div><strong>${escape(preset.name)}</strong><p>${escape(preset.description)}</p></div>${preset.id.startsWith("builtin-") ? "<span>Built in</span>" : `<div class="ccresdoc-source-manager__actions"><button type="button" data-action="edit-preset" data-preset="${escape(preset.id)}">Edit</button><button type="button" data-action="delete-preset" data-preset="${escape(preset.id)}" data-danger>Delete</button></div>`}</div>`).join("")}</div></section>
      <div class="ccresdoc-source-manager__toolbar"><span>${dirty ? "Unsaved draft" : snapshot?.pendingApply ? "Saved · not active" : "Saved · active"}</span><div class="ccresdoc-source-manager__actions">${conflict ? `<button type="button" data-action="reload">Reload</button><button type="button" data-action="rebase">Rebase my changes…</button>` : ""}<button type="button" data-action="discard">Discard draft</button><button type="button" data-action="save" data-primary>Save</button><button type="button" data-action="apply">Apply saved</button></div></div>`;
    root.querySelectorAll<HTMLDetailsElement>("details[data-rules]").forEach((item) => { item.open = expanded.has(item.dataset.rules ?? ""); });
    renderErrors(); updateActions();
  };
  const validate = async () => {
    if (!snapshot) return;
    const token = ++validateToken;
    diagnostics = localCatalogErrors(draft);
    renderErrors();
    try {
      const native = await api.validate(draft, snapshot.runtimeGeneration);
      if (token !== validateToken || stopped) return;
      diagnostics = [...diagnostics, ...native.filter((entry) => !diagnostics.some((local) => local.field === entry.field && local.message === entry.message))];
      renderErrors();
    } catch (issue) { if (token === validateToken) setMessage(errorMessage(issue), true); }
  };
  const accept = (next: EditorSnapshot) => {
    snapshot = next; baseCatalog = cloneCatalog(next.catalog); draft = cloneCatalog(next.catalog);
    dirty = false; conflict = false; diagnostics = next.validation;
    setMessage(next.runtimeError ?? (next.pendingApply ? "Saved settings are not active. Choose Apply saved when ready." : "Document sources loaded."), Boolean(next.runtimeError));
    render();
  };
  const reload = async (detectConflict = false) => {
    try {
      const next = await api.get();
      if (stopped) return;
      if (detectConflict && dirty && next.revision !== snapshot?.revision) {
        conflict = true; setMessage("Settings changed in another window. Reload or rebase this draft.", true); render(); return;
      }
      if (!detectConflict || !dirty) accept(next);
    } catch (issue) { setMessage(errorMessage(issue), true); }
  };
  const save = async (rebase = false) => {
    if (!snapshot || busy) return;
    await validate();
    if (diagnostics.some((entry) => entry.blocking !== false)) { setMessage("Correct the highlighted source fields before saving.", true); return; }
    busy = true; updateActions();
    try {
      const next = rebase && snapshot.revision
        ? await api.rebase(draft, baseCatalog, snapshot.revision, snapshot.runtimeGeneration)
        : await api.save(draft, snapshot.revision, snapshot.runtimeGeneration);
      accept(next); setMessage("Sources saved. Choose Apply saved to update documents and search.");
    } catch (issue) {
      const code = issue && typeof issue === "object" && "code" in issue ? String(issue.code) : "";
      if (code === "revision_conflict") conflict = true;
      setMessage(errorMessage(issue), true); render();
    } finally { busy = false; updateActions(); }
  };
  const apply = async () => {
    if (!snapshot || dirty || busy) return;
    busy = true; updateActions(); setMessage("Applying saved sources…");
    try {
      const result = await api.apply(snapshot.revision, snapshot.runtimeGeneration);
      if (stopped) return;
      if (window.location.pathname === "/docs/ccresdoc/document-sources/") await reload();
      setMessage(result.status === "active" ? "Sources are active. Browse the normal documents and search." : "Saved sources are not active. Correct the source and retry Apply.", result.status !== "active");
    } catch (issue) { setMessage(errorMessage(issue), true); }
    finally { busy = false; updateActions(); }
  };
  const setField = (element: HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement) => {
    const source = findSource(element.dataset.source ?? "");
    if (!source) return;
    const value = element.type === "checkbox" ? (element as HTMLInputElement).checked : element.value;
    switch (element.dataset.field) {
      case "label": source.label = String(value); break;
      case "enabled": source.enabled = Boolean(value); break;
      case "root": source.root = String(value); break;
      case "mount": source.mount = String(value); break;
      case "scan_kind": source.scan_root = value === "separate" ? { kind: "separate", path: source.scan_root.kind === "separate" ? source.scan_root.path : "" } : { kind: "same" }; break;
      case "scan_root": source.scan_root = { kind: "separate", path: String(value) }; break;
      case "rules.format": if (source.rules.kind === "custom") source.rules.format = value as "markdown" | "code" | "skill"; break;
      case "rules.include": if (source.rules.kind === "custom") source.rules.include = lines(String(value)); break;
      case "rules.exclude": if (source.rules.kind === "custom") source.rules.exclude = lines(String(value)); break;
    }
    updateDirty();
    if (element.dataset.field === "label") {
      const heading = root.querySelector<HTMLElement>(`[data-source-card="${CSS.escape(source.id)}"] .ccresdoc-source-manager__source-head strong`);
      if (heading) heading.textContent = source.label || "Untitled source";
    }
    if (element.dataset.field === "enabled" || element.dataset.field === "scan_kind") {
      const fieldName = element.dataset.field;
      render();
      root.querySelector<HTMLElement>(`[data-source="${CSS.escape(source.id)}"][data-field="${fieldName}"]`)?.focus();
    }
    void validate();
  };
  const showDialog = (kind: "new" | "edit" | "save", source?: Source, preset?: Preset) => {
    const current = preset ?? { id: "", name: "", description: "", version: 1, rules: { kind: "custom" as const, definition_version: 1, format: "markdown" as const, include: ["**/*.md"], exclude: [] } };
    dialog.innerHTML = `<form method="dialog"><h2>${kind === "save" ? "Save rules as preset" : kind === "edit" ? "Edit preset" : "New preset"}</h2><p>Presets have reading rules without source folders, names or URL paths.</p><label>Name<input name="name" required value="${escape(current.name)}"></label><label>Description<input name="description" value="${escape(current.description)}"></label>${kind === "save" ? "<p>The source will link to this preset; its current rules and folder stay the same.</p>" : current.rules.kind !== "custom" ? `<p>Rules: ${escape(rulesSummary(current.rules))}. Specialized renderer rules stay intact; edit the name and description here.</p>` : `<label>Format<select name="format"><option value="markdown">Markdown</option><option value="code">Code and text</option><option value="skill">Skill packages</option></select></label><label>Include patterns<textarea name="include" required aria-describedby="preset-include-error">${escape(current.rules.include.join("\n"))}</textarea><small id="preset-include-error" role="alert" hidden></small></label><label>Exclude patterns<textarea name="exclude">${escape(current.rules.exclude.join("\n"))}</textarea></label>`}<div class="ccresdoc-source-manager__dialog-actions"><button type="button" data-dialog-cancel>Cancel</button><button type="submit" data-primary>Save preset</button></div></form>`;
    const form = dialog.querySelector("form")!;
    const format = form.elements.namedItem("format") as HTMLSelectElement | null;
    if (format && current.rules.kind === "custom") format.value = current.rules.format;
    const include = form.elements.namedItem("include") as HTMLTextAreaElement | null;
    const includeError = form.querySelector<HTMLElement>("#preset-include-error");
    const clearIncludeError = () => {
      if (includeError) { includeError.textContent = ""; includeError.hidden = true; }
      include?.removeAttribute("aria-invalid");
    };
    include?.addEventListener("input", clearIncludeError);
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      const data = new FormData(form);
      const name = String(data.get("name") ?? "").trim();
      if (!name) return;
      const description = String(data.get("description") ?? "").trim();
      if (kind === "save" && source) {
        const result = saveRulesAsPreset(source, id(), name, description);
        Object.assign(source, result.source); draft.presets.push(result.preset);
      } else {
        const rules = current.rules.kind === "custom"
          ? { kind: "custom" as const, definition_version: current.rules.definition_version + (kind === "edit" ? 1 : 0), format: String(data.get("format")) as "markdown" | "code" | "skill", include: lines(String(data.get("include") ?? "")), exclude: lines(String(data.get("exclude") ?? "")) }
          : structuredClone(current.rules);
        if (rules.kind === "custom" && !rules.include.length) {
          if (includeError) { includeError.textContent = "Enter at least one include pattern."; includeError.hidden = false; }
          include?.setAttribute("aria-invalid", "true");
          include?.focus();
          return;
        }
        if (kind === "edit" && preset) { preset.name = name; preset.description = description; preset.version += 1; preset.rules = rules; }
        else draft.presets.push({ id: id(), name, description, version: 1, rules });
      }
      dialog.close(); updateDirty(); render(); void validate();
    });
    dialog.querySelector("[data-dialog-cancel]")?.addEventListener("click", () => dialog.close(), { once: true });
    dialog.showModal();
    (form.elements.namedItem("name") as HTMLInputElement)?.focus();
  };
  const onClick = async (event: MouseEvent) => {
    const button = (event.target as Element).closest<HTMLButtonElement>("button[data-action]");
    if (!button || !root.contains(button)) return;
    const action = button.dataset.action;
    const source = findSource(button.dataset.source ?? "");
    const preset = findPreset(button.dataset.preset ?? "");
    if (action === "add-source") { const chosen = findPreset((root.querySelector("#manager-add-preset") as HTMLSelectElement).value); if (!chosen) return; draft.sources.push(sourceFromPreset(draft, chosen, id())); updateDirty(); render(); }
    if (action === "duplicate" && source) { draft.sources.push(duplicateSource(draft, source, id())); updateDirty(); render(); }
    if (action === "remove" && source && confirm(`Remove ${source.label} from CCResDoc? Its original files will stay in place.`)) { draft.sources = draft.sources.filter((entry) => entry.id !== source.id); updateDirty(); render(); }
    if ((action === "pick-root" || action === "pick-scan") && source && snapshot) { try { const selected = await api.pick(snapshot.runtimeGeneration); if (selected) { if (action === "pick-root") source.root = selected; else source.scan_root = { kind: "separate", path: selected }; updateDirty(); render(); void validate(); } } catch (issue) { setMessage(errorMessage(issue), true); } }
    if (action === "save-rules-preset" && source) showDialog("save", source);
    if (action === "new-preset") showDialog("new");
    if (action === "edit-preset" && preset && !preset.id.startsWith("builtin-")) showDialog("edit", undefined, preset);
    if (action === "delete-preset" && preset && !preset.id.startsWith("builtin-") && confirm(`Delete ${preset.name}? Existing source snapshots will stay unchanged.`)) { draft.presets = draft.presets.filter((entry) => entry.id !== preset.id); updateDirty(); render(); }
    if (action === "use-preset" && source) { const chosen = findPreset(root.querySelector<HTMLSelectElement>(`[data-preset-choice="${CSS.escape(source.id)}"]`)?.value ?? ""); if (chosen && confirm(`Replace draft rules for ${source.label}?\n\nCurrent: ${rulesSummary(source.rules)}\nNew: ${rulesSummary(chosen.rules)}\n\nFolder, name, URL and enabled state stay unchanged.`)) { Object.assign(source, applyPresetRules(source, chosen)); updateDirty(); render(); void validate(); } }
    if (action === "save") void save();
    if (action === "apply") void apply();
    if (action === "discard" && (!dirty || confirm("Discard unsaved source and preset changes?"))) { draft = cloneCatalog(baseCatalog); dirty = false; conflict = false; render(); void validate(); }
    if (action === "reload" && confirm("Discard this draft and reload settings from disk?")) void reload();
    if (action === "rebase" && snapshot?.revision && confirm("Rebase only your changed sources and presets onto the latest settings?")) void save(true);
  };
  const onInput = (event: Event) => { const target = event.target; if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement) if (target.dataset.field) setField(target); };
  const onFocus = () => { if (!busy && snapshot) void reload(true); };
  const onBeforeUnload = (event: BeforeUnloadEvent) => { if (!dirty) return; event.preventDefault(); event.returnValue = ""; };
  const stopNavigationGuard = installUnsavedNavigationGuard({
    win: window, doc: document, isDirty: () => dirty,
    confirmLeave: () => confirm("Leave the source editor and discard unsaved changes?"),
  });
  root.addEventListener("click", onClick);
  root.addEventListener("input", onInput);
  root.addEventListener("change", onInput);
  window.addEventListener("focus", onFocus);
  window.addEventListener("beforeunload", onBeforeUnload);
  void reload();
  return () => { stopped = true; root.removeEventListener("click", onClick); root.removeEventListener("input", onInput); root.removeEventListener("change", onInput); window.removeEventListener("focus", onFocus); window.removeEventListener("beforeunload", onBeforeUnload); stopNavigationGuard(); dialog.remove(); };
}

export function DocumentSourcesManager() {
  getScope().onActivate(() => {
    const root = document.querySelector<HTMLElement>("[data-ccresdoc-source-manager]");
    return root ? startDocumentSourcesManager(root) : undefined;
  });
  return <div class="ccresdoc-source-manager" data-ccresdoc-source-manager><p>Loading document sources…</p></div>;
}

DocumentSourcesManager.displayName = "DocumentSourcesManager";
