import { afterEach, describe, expect, it, vi } from "vitest";
import { startDocumentSourcesManager } from "../src/document-sources/manager";
import { sourceFromPreset, type Catalog, type Preset } from "../src/document-sources/catalog";

afterEach(() => { vi.unstubAllGlobals(); });

describe("document source preset dialog", () => {
  it("keeps an invalid include draft available for correction and saves it on retry", async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === "get_source_editor_snapshot") return {
        catalog: { sources: [], presets: [] }, builtinPresets: [], revision: "initial",
        loadStatus: "valid", validation: [], runtimePhase: "ready", runtimeGeneration: 1,
        activeSourceIds: [], runtimeError: null, pendingApply: false,
      };
      if (command === "validate_source_editor_draft") return [];
      throw new Error(`Unexpected command: ${command}`);
    });
    vi.stubGlobal("__TAURI__", { core: { invoke } });
    const root = document.createElement("div");
    document.body.append(root);
    const stop = startDocumentSourcesManager(root);
    try {
      await vi.waitFor(() => expect(root.querySelector("[data-action='new-preset']")).not.toBeNull());
      root.querySelector<HTMLButtonElement>("[data-action='new-preset']")!.click();
      const dialog = document.querySelector<HTMLDialogElement>("dialog.ccresdoc-source-manager__dialog")!;
      const form = dialog.querySelector("form")!;
      const name = form.elements.namedItem("name") as HTMLInputElement;
      const include = form.elements.namedItem("include") as HTMLTextAreaElement;
      name.value = "My preset";
      include.value = "   \n  ";
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
      expect(dialog.open).toBe(true);
      expect(include.value).toBe("   \n  ");
      expect(include.getAttribute("aria-invalid")).toBe("true");
      expect(dialog.querySelector("#preset-include-error")?.textContent).toBe("Enter at least one include pattern.");
      expect(root.textContent).not.toContain("My preset");

      include.value = "**/*.md";
      include.dispatchEvent(new Event("input", { bubbles: true }));
      expect(include.hasAttribute("aria-invalid")).toBe(false);
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
      expect(dialog.open).toBe(false);
      expect(root.textContent).toContain("My preset");
      expect(root.querySelector("[data-action='save']")?.hasAttribute("disabled")).toBe(false);
    } finally {
      stop();
    }
  });

  it("locks draft controls while saving so later input cannot be discarded", async () => {
    const preset: Preset = { id: "builtin-markdown", name: "Markdown", description: "", version: 1, rules: { kind: "custom", definition_version: 1, format: "markdown", include: ["**/*.md"], exclude: [] } };
    const catalog: Catalog = { sources: [sourceFromPreset({ sources: [], presets: [] }, preset, "source-one")], presets: [] };
    const initial = { catalog, builtinPresets: [preset], revision: "initial", loadStatus: "valid", validation: [], runtimePhase: "ready", runtimeGeneration: 1, activeSourceIds: [], runtimeError: null, pendingApply: false };
    let completeSave!: (value: typeof initial) => void;
    const pendingSave = new Promise<typeof initial>((resolve) => { completeSave = resolve; });
    const invoke = vi.fn(async (command: string, args?: { catalog?: Catalog }) => {
      if (command === "get_source_editor_snapshot") return initial;
      if (command === "validate_source_editor_draft") return [];
      if (command === "save_source_editor_draft") return pendingSave;
      throw new Error(`Unexpected command: ${command} ${JSON.stringify(args)}`);
    });
    vi.stubGlobal("__TAURI__", { core: { invoke } });
    const root = document.createElement("div");
    document.body.append(root);
    const stop = startDocumentSourcesManager(root);
    try {
      await vi.waitFor(() => expect(root.querySelector<HTMLInputElement>("[data-field='label']")).not.toBeNull());
      const label = root.querySelector<HTMLInputElement>("[data-field='label']")!;
      label.value = "Saved name";
      label.dispatchEvent(new Event("input", { bubbles: true }));
      root.querySelector<HTMLButtonElement>("[data-action='save']")!.click();
      await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("save_source_editor_draft", expect.anything()));
      expect(root.inert).toBe(true);
      root.querySelector<HTMLButtonElement>("[data-action='add-source']")!.click();
      expect(root.querySelectorAll("[data-source-card]")).toHaveLength(1);
      label.value = "Later input";
      label.dispatchEvent(new Event("input", { bubbles: true }));
      const savedCatalog = structuredClone(catalog);
      savedCatalog.sources[0].label = "Saved name";
      completeSave({ ...initial, catalog: savedCatalog, revision: "saved" });
      await vi.waitFor(() => expect(root.inert).toBe(false));
      expect(root.querySelector<HTMLInputElement>("[data-field='label']")?.value).toBe("Saved name");
      const saveCall = invoke.mock.calls.find(([command]) => command === "save_source_editor_draft");
      expect(saveCall?.[1]?.catalog?.sources[0].label).toBe("Saved name");
    } finally {
      stop();
    }
  });
});
