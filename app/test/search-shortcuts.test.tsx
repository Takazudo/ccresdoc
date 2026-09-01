import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import {
  SEARCH_COMMAND_EVENT,
  SEARCH_WIDGET_SCRIPT,
  closeSearch,
  openSearch,
  refreshSearch,
} from "@takazudo/zudo-doc/search-widget-script";

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

function seedSearchWidget(disableBuiltInShortcut = true) {
  const search = document.createElement("site-search");
  search.dataset.base = "/docs/";
  search.dataset.resultCountTemplate = "{count} results";
  if (disableBuiltInShortcut) search.dataset.disableBuiltInShortcut = "true";
  search.innerHTML = `
    <button data-open-search type="button">Open</button>
    <dialog data-search-dialog>
      <input data-search-input type="search" autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck="false">
      <span data-search-count></span><span data-search-count-narrow></span>
      <button data-close-search type="button">Close</button>
      <div data-search-results><p data-search-placeholder>Search</p></div>
    </dialog>`;
  document.body.append(search);
  return {
    search,
    dialog: search.querySelector("dialog") as HTMLDialogElement,
    input: search.querySelector("input") as HTMLInputElement,
  };
}

beforeAll(() => {
  Function(SEARCH_WIDGET_SCRIPT)();
});

afterEach(() => {
  document.body.replaceChildren();
  document.documentElement.style.overflow = "";
  vi.restoreAllMocks();
});

describe("controlled search commands", () => {
  it("opens and refreshes through the public command seam", async () => {
    const fetch = vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(JSON.stringify([])));
    const { dialog, input } = seedSearchWidget();

    openSearch({ refresh: true });
    await flush();

    expect(dialog.open).toBe(true);
    expect(document.activeElement).toBe(input);
    expect(fetch).toHaveBeenCalledWith("/docs/search-index.json");

    expect(() => openSearch({ refresh: false })).not.toThrow();

    refreshSearch();
    await flush();
    expect(fetch).toHaveBeenCalledTimes(2);

    closeSearch();
    expect(dialog.open).toBe(false);
    expect(() => closeSearch()).not.toThrow();
  });

  it("ignores an older in-flight index response after a controlled refresh", async () => {
    let resolveStale!: (response: Response) => void;
    const stale = new Promise<Response>((resolve) => { resolveStale = resolve; });
    const fetch = vi.spyOn(globalThis, "fetch")
      .mockReturnValueOnce(stale)
      .mockResolvedValueOnce(new Response(JSON.stringify([
        { title: "Fresh result", description: "current", body: "", url: "/docs/fresh" },
      ])));
    const { input, search } = seedSearchWidget();

    openSearch({ refresh: true });
    refreshSearch();
    await flush();
    expect(fetch).toHaveBeenCalledTimes(2);
    resolveStale(new Response(JSON.stringify([
      { title: "Stale result", description: "old", body: "", url: "/docs/stale" },
    ])));
    await flush();

    input.value = "Fresh";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await new Promise((resolve) => setTimeout(resolve, 180));
    expect(search.querySelector("[data-search-results]")?.textContent).toContain("Fresh result");
    expect(search.querySelector("[data-search-results]")?.textContent).not.toContain("Stale result");
  });

  it.each(["ctrlKey", "metaKey"] as const)(
    "disables the stale package shortcut while preserving controlled open (%s)",
    async (modifier) => {
      vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(JSON.stringify([])));
      const { dialog } = seedSearchWidget(true);
      const event = new KeyboardEvent("keydown", {
        key: "k",
        [modifier]: true,
        bubbles: true,
        cancelable: true,
      });
      document.dispatchEvent(event);
      expect(event.defaultPrevented).toBe(false);
      expect(dialog.open).toBe(false);

      openSearch({ refresh: false });
      await flush();
      expect(dialog.open).toBe(true);
    },
  );

  it("removes its public command listener and restores document overflow on disconnect", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(JSON.stringify([])));
    const { search, dialog } = seedSearchWidget();
    openSearch({ refresh: false });
    await flush();
    expect(document.documentElement.style.overflow).toBe("hidden");
    search.remove();
    document.dispatchEvent(new CustomEvent(SEARCH_COMMAND_EVENT, {
      detail: { action: "open", refresh: true },
    }));
    expect(dialog.open).toBe(false);
    expect(document.documentElement.style.overflow).toBe("");
  });
});
