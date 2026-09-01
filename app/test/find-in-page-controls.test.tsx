/** @jsxRuntime automatic */
/** @jsxImportSource preact */

import { act } from "preact/test-utils";
import { render } from "preact";
import { afterEach, describe, expect, it } from "vitest";
import {
  FindInPageInit,
  closeFindInPage,
  createFindInPage,
  openFindInPage,
} from "@takazudo/zudo-doc/find-in-page";

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

afterEach(() => {
  delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  document.body.replaceChildren();
});

describe("controlled Find in Page", () => {
  it("opens through the public seam with helper-free search and icon controls", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    act(() => render(<FindInPageInit disableBuiltInShortcut />, document.body));
    await act(async () => { await flush(); });
    act(() => openFindInPage());

    const input = document.querySelector<HTMLInputElement>('input[aria-label="Find in page"]');
    expect(input).not.toBeNull();
    expect(input?.type).toBe("search");
    expect(input?.getAttribute("autocomplete")).toBe("off");
    expect(input?.getAttribute("autocorrect")).toBe("off");
    expect(input?.getAttribute("autocapitalize")).toBe("off");
    expect(input?.getAttribute("spellcheck")).toBe("false");
    expect([...document.querySelectorAll(".find-in-page-control")].map((item) => item.getAttribute("aria-label"))).toEqual([
      "Previous match (Shift+Enter)",
      "Next match (Enter)",
      "Close find in page (Escape)",
    ]);

    input?.blur();
    act(() => openFindInPage());
    expect(document.activeElement).toBe(input);

    act(() => closeFindInPage());
    expect(document.querySelector('[data-find-in-page-bar]')).toBeNull();
    act(() => render(null, document.body));
  });

  it.each(["ctrlKey", "metaKey"] as const)("opts out of the built-in shortcut (%s)", async (modifier) => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    act(() => render(<FindInPageInit disableBuiltInShortcut />, document.body));
    await act(async () => { await flush(); });
    const event = new KeyboardEvent("keydown", { key: "f", [modifier]: true, bubbles: true, cancelable: true });
    document.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
    expect(document.querySelector('[data-find-in-page-bar]')).toBeNull();
    act(() => render(null, document.body));
  });

  it("filters hidden, chrome, script, control, and widget subtrees and restores content idempotently", () => {
    const article = document.createElement("article");
    article.innerHTML = `
      <p>Visible needle and needle.</p>
      <p hidden>hidden needle</p>
      <p inert>inert needle</p>
      <p aria-hidden="true">aria needle</p>
      <p style="display:none">display needle</p>
      <dialog>closed dialog needle</dialog>
      <details><summary>visible summary</summary><p>closed details needle</p></details>
      <script>needle</script><style>.needle { color: red }</style><template>needle</template>
      <button>control needle</button><div role="button">widget needle</div>
      <div data-ccresdoc-browser-toolbar-shell>chrome needle</div>`;
    const original = article.innerHTML;
    document.body.append(article);
    const find = createFindInPage();

    expect(find.find(article, "needle")).toEqual({ matches: 2, activeMatchOrdinal: 1 });
    expect(find.next()).toEqual({ matches: 2, activeMatchOrdinal: 2 });
    expect(find.prev()).toEqual({ matches: 2, activeMatchOrdinal: 1 });
    find.stop();
    find.stop();
    expect(article.querySelectorAll("[data-find-match]")).toHaveLength(0);
    expect(article.textContent).toContain("Visible needle and needle.");
    expect(article.innerHTML).toBe(original);

    expect(find.find(article, "missing")).toEqual({ matches: 0, activeMatchOrdinal: 0 });
    expect(article.querySelectorAll("[data-find-match]")).toHaveLength(0);
  });

  it("cleans matches on route preparation and component teardown", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const article = document.createElement("article");
    article.className = "zd-content";
    article.textContent = "route needle";
    document.body.append(article);
    const mount = document.createElement("div");
    document.body.append(mount);
    act(() => render(<FindInPageInit disableBuiltInShortcut />, mount));
    await act(async () => { await flush(); });
    act(() => openFindInPage());
    const input = document.querySelector<HTMLInputElement>('input[aria-label="Find in page"]')!;
    input.value = "needle";
    act(() => { input.dispatchEvent(new Event("input", { bubbles: true })); });
    expect(article.querySelectorAll("[data-find-match]")).toHaveLength(1);

    act(() => { document.dispatchEvent(new Event("zfb:before-preparation")); });
    expect(article.querySelectorAll("[data-find-match]")).toHaveLength(0);
    expect(document.querySelector('[data-find-in-page-bar]')).toBeNull();

    act(() => render(null, mount));
    expect(article.querySelectorAll("[data-find-match]")).toHaveLength(0);
  });
});
