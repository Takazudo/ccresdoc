import { afterEach, describe, expect, it, vi } from "vitest";
import { CCResDocBrowserAdapter } from "../src/browser-chrome/adapter";
import type { BrowserBootstrap, BrowserCommandEnvelope } from "../src/browser-chrome/types";

type TestWindow = Window & {
  __TAURI__?: {
    core?: { invoke?: <T>(command: string, args?: Record<string, unknown>) => Promise<T> };
    event?: { listen?: <T>(event: string, listener: (event: { payload: T }) => void) => Promise<() => void> };
  };
};

const testWindow = window as TestWindow;
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

afterEach(() => {
  delete testWindow.__TAURI__;
  sessionStorage.clear();
  document.body.replaceChildren();
  history.replaceState({}, "", "/docs/");
  vi.restoreAllMocks();
});

describe("CCResDoc browser adapter", () => {
  it("uses manifest defaults in browser mode without enabling host commands", () => {
    history.replaceState({}, "", "/docs/");
    const adapter = new CCResDocBrowserAdapter(testWindow);
    const stop = adapter.start();
    expect(adapter.getSnapshot()).toMatchObject({ mode: "browser", bootstrap: "ready", stablePath: "/docs/" });
    expect(adapter.getSnapshot().availability["find-in-page"]).toBe(true);
    expect(adapter.getSnapshot().availability["reload-documentation"]).toBe(false);
    expect(adapter.getSnapshot().availability.settings).toBe(false);
    stop();
  });

  it("waits for Tauri bootstrap, skips native-owned keys, and deduplicates native envelopes", async () => {
    const callbacks = new Map<string, (event: { payload: unknown }) => void>();
    const search = document.createElement("site-search");
    const command = vi.fn();
    const findCommand = vi.fn();
    document.addEventListener("zudo-doc:search-command", command);
    document.addEventListener("zudo-doc:find-in-page-command", findCommand);
    document.body.append(search);
    const bootstrap: BrowserBootstrap = {
      shortcutEntries: [{ commandId: "search-documentation", bindings: ["Mod+K"] }],
      nativeOwnedBindings: [{ commandId: "search-documentation", binding: "Mod+K" }],
      hostCapabilities: { reloadDocumentation: true, openInDefaultBrowser: true },
      runtimeGeneration: 42,
    };
    const invoke = vi.fn(async (command: string) => {
      if (command === "get_browser_bootstrap") return bootstrap;
      return undefined;
    });
    testWindow.__TAURI__ = {
      core: { invoke: invoke as any },
      event: { listen: async (event, listener) => { callbacks.set(event, listener as (event: { payload: unknown }) => void); return () => undefined; } },
    };

    const adapter = new CCResDocBrowserAdapter(testWindow);
    const stop = adapter.start();
    expect(adapter.getSnapshot().bootstrap).toBe("loading");
    await flush();
    expect(adapter.getSnapshot()).toMatchObject({ mode: "tauri", bootstrap: "ready" });
    expect(adapter.getSnapshot().availability.settings).toBe(true);

    const key = new KeyboardEvent("keydown", { key: "k", metaKey: true, bubbles: true, cancelable: true });
    document.body.dispatchEvent(key);
    expect(key.defaultPrevented).toBe(false);
    expect(command).not.toHaveBeenCalled();

    const envelope: BrowserCommandEnvelope = {
      commandId: "search-documentation", origin: "native_menu", invocationId: 5,
      runtimeGeneration: 42, hostHandled: false,
    };
    callbacks.get("ccresdoc://browser-command")?.({ payload: envelope });
    callbacks.get("ccresdoc://browser-command")?.({ payload: envelope });
    await flush();
    expect(command).toHaveBeenCalledOnce();
    expect((command.mock.calls[0]?.[0] as CustomEvent).detail).toEqual({ action: "open", refresh: true });
    callbacks.get("ccresdoc://browser-command")?.({ payload: {
      commandId: "find-in-page", origin: "native_menu", invocationId: 6,
      runtimeGeneration: 42, hostHandled: false,
    } satisfies BrowserCommandEnvelope });
    await flush();
    expect(findCommand).toHaveBeenCalledOnce();
    expect((findCommand.mock.calls[0]?.[0] as CustomEvent).detail).toEqual({ action: "open" });
    document.removeEventListener("zudo-doc:search-command", command);
    document.removeEventListener("zudo-doc:find-in-page-command", findCommand);
    stop();
  });

  it("fails closed for keyboard and host actions when Tauri bootstrap fails", async () => {
    testWindow.__TAURI__ = {
      core: { invoke: vi.fn(async () => { throw new Error("offline"); }) as any },
      event: { listen: async () => () => undefined },
    };
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const adapter = new CCResDocBrowserAdapter(testWindow);
    const stop = adapter.start();
    await flush();
    expect(adapter.getSnapshot().bootstrap).toBe("unavailable");
    expect(await adapter.execute({ commandId: "reload-documentation", origin: "toolbar", invocationId: 1 })).toBe(false);
    expect(consoleError).toHaveBeenCalled();
    stop();
  });
});
