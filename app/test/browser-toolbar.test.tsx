import { describe, expect, it, vi } from "vitest";
import { mountView, settle } from "./helpers/zudo-react";
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

const extras: HTMLElement[] = [];
function mount() {
  const execute = vi.fn(async (_envelope: BrowserCommandEnvelope) => true);
  const adapter: BrowserToolbarAdapter = {
    getSnapshot: () => snapshot,
    subscribe: () => () => undefined,
    start: () => () => undefined,
    execute,
  };
  const { container: root } = mountView(BrowserToolbarView, { adapter });
  return { root, execute };
}

describe("browser toolbar", () => {
  it("renders the neutral snapshot with accessible actions and path", () => {
    const { root } = mount();
    expect(root.querySelector("nav")?.getAttribute("aria-label")).toBe("Browser navigation");
    expect((root.querySelector("input") as HTMLInputElement).value).toBe("/docs/codex/");
    expect(root.querySelector<HTMLButtonElement>("[data-browser-command='back']")?.disabled).toBe(false);
    expect(root.querySelector<HTMLButtonElement>("[data-browser-command='forward']")?.disabled).toBe(true);
    expect(root.querySelector<HTMLButtonElement>("[data-browser-command='reload-documentation']")?.disabled).toBe(true);
    expect(root.querySelector("[data-browser-command='more']")?.getAttribute("aria-haspopup")).toBe("menu");
    expect([...root.querySelectorAll<HTMLElement>("[role='menuitem']")].map(
      (item) => item.dataset.browserCommand,
    )).toEqual([
      "search-documentation",
      "copy-page-path",
      "open-in-default-browser",
      "settings",
    ]);
  });

  it("opens More with ArrowDown, moves focus, and restores it on Escape", async () => {
    const { root } = mount();
    const trigger = root.querySelector<HTMLButtonElement>("[data-browser-command='more']")!;
    trigger.focus();
    trigger.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
    await settle();
    const menu = root.querySelector<HTMLElement>("[role='menu']")!;
    expect(menu.hidden).toBe(false);
    expect(document.activeElement?.getAttribute("data-browser-command")).toBe("search-documentation");

    document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await settle();
    expect(menu.hidden).toBe(true);
    expect(document.activeElement).toBe(trigger);
  });

  it("opens More with ArrowUp focused on the last available item", async () => {
    const { root } = mount();
    const trigger = root.querySelector<HTMLButtonElement>("[data-browser-command='more']")!;
    trigger.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true, cancelable: true }));
    await settle();
    expect(document.activeElement?.getAttribute("data-browser-command")).toBe("copy-page-path");
  });

  it("closes More on an outside press even when the target stops propagation", async () => {
    const { root } = mount();
    const trigger = root.querySelector<HTMLButtonElement>("[data-browser-command='more']")!;
    const menu = root.querySelector<HTMLElement>("[role='menu']")!;
    const outside = document.createElement("button");
    outside.addEventListener("pointerdown", (event) => event.stopPropagation());
    document.body.append(outside);
    extras.push(outside);

    trigger.click();
    await settle();
    expect(menu.hidden).toBe(false);

    outside.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await settle();
    expect(menu.hidden).toBe(true);
  });

  it("routes direct and overflow actions through the same adapter", async () => {
    const { root, execute } = mount();
    root.querySelector<HTMLButtonElement>("[data-browser-command='back']")!.click();
    root.querySelector<HTMLButtonElement>("[data-browser-command='more']")!.click();
    await settle();
    root.querySelector<HTMLButtonElement>("[data-browser-command='copy-page-path'][role='menuitem']")!.click();
    expect((execute.mock.calls[0]?.[0] as BrowserCommandEnvelope)).toMatchObject({ commandId: "back", origin: "toolbar" });
    expect((execute.mock.calls[1]?.[0] as BrowserCommandEnvelope)).toMatchObject({ commandId: "copy-page-path", origin: "overflow" });
  });
});
