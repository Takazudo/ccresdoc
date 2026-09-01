/** @jsxRuntime automatic */
/** @jsxImportSource preact */

import { act } from "preact/test-utils";
import { render } from "preact";
import { afterEach, describe, expect, it, vi } from "vitest";
import { BrowserToolbarView } from "../src/browser-chrome/toolbar";
import type { BrowserCommandEnvelope, BrowserToolbarAdapter, BrowserToolbarSnapshot } from "../src/browser-chrome/types";

const snapshot: BrowserToolbarSnapshot = {
  title: "Codex Resources",
  stablePath: "/docs/codex/",
  canGoBack: true,
  canGoForward: false,
  traversalPending: false,
  mode: "browser",
  bootstrap: "ready",
  availability: {
    back: true, forward: true, home: true, "reload-documentation": false,
    "find-in-page": true, "search-documentation": true, "copy-page-path": true,
    "open-in-default-browser": false, settings: false,
  },
};

const roots: HTMLElement[] = [];
function mount() {
  const execute = vi.fn(async (_envelope: BrowserCommandEnvelope) => true);
  const adapter: BrowserToolbarAdapter = {
    getSnapshot: () => snapshot,
    subscribe: () => () => undefined,
    start: () => () => undefined,
    execute,
  };
  const root = document.createElement("div");
  document.body.append(root); roots.push(root);
  act(() => render(<BrowserToolbarView adapter={adapter} />, root));
  return { root, execute };
}

afterEach(() => {
  for (const root of roots.splice(0)) { act(() => render(null, root)); root.remove(); }
});

describe("browser toolbar", () => {
  it("renders the neutral snapshot with accessible actions and path", () => {
    const { root } = mount();
    expect(root.querySelector("nav")?.getAttribute("aria-label")).toBe("Browser navigation");
    expect((root.querySelector("input") as HTMLInputElement).value).toBe("/docs/codex/");
    expect(root.querySelector<HTMLButtonElement>("[data-browser-command='back']")?.disabled).toBe(false);
    expect(root.querySelector<HTMLButtonElement>("[data-browser-command='forward']")?.disabled).toBe(true);
    expect(root.querySelector<HTMLButtonElement>("[data-browser-command='reload-documentation']")?.disabled).toBe(true);
    expect(root.querySelector("[data-browser-command='more']")?.getAttribute("aria-haspopup")).toBe("menu");
  });

  it("opens More with ArrowDown, moves focus, and restores it on Escape", async () => {
    const { root } = mount();
    const trigger = root.querySelector<HTMLButtonElement>("[data-browser-command='more']")!;
    trigger.focus();
    act(() => { trigger.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true })); });
    await Promise.resolve();
    const menu = root.querySelector<HTMLElement>("[role='menu']")!;
    expect(menu.hidden).toBe(false);
    expect(document.activeElement?.getAttribute("data-browser-command")).toBe("search-documentation");

    act(() => { document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true })); });
    await Promise.resolve();
    expect(menu.hidden).toBe(true);
    expect(document.activeElement).toBe(trigger);
  });

  it("opens More with ArrowUp focused on the last available item", async () => {
    const { root } = mount();
    const trigger = root.querySelector<HTMLButtonElement>("[data-browser-command='more']")!;
    act(() => { trigger.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true, cancelable: true })); });
    await Promise.resolve();
    expect(document.activeElement?.getAttribute("data-browser-command")).toBe("copy-page-path");
  });

  it("routes direct and overflow actions through the same adapter", () => {
    const { root, execute } = mount();
    root.querySelector<HTMLButtonElement>("[data-browser-command='back']")!.click();
    root.querySelector<HTMLButtonElement>("[data-browser-command='more']")!.click();
    root.querySelector<HTMLButtonElement>("[data-browser-command='copy-page-path'][role='menuitem']")!.click();
    expect((execute.mock.calls[0]?.[0] as BrowserCommandEnvelope)).toMatchObject({ commandId: "back", origin: "toolbar" });
    expect((execute.mock.calls[1]?.[0] as BrowserCommandEnvelope)).toMatchObject({ commandId: "copy-page-path", origin: "overflow" });
  });
});
