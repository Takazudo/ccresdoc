import { afterEach, describe, expect, it, vi } from "vitest";
import { startDocumentSourcesManager } from "../src/document-sources/manager";

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
});
