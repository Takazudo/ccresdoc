import { act } from "preact/test-utils";
import { render } from "preact";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AppearanceBridge } from "../src/appearance/bridge";

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("docs appearance bridge", () => {
  afterEach(() => {
    delete (globalThis as any).__TAURI__;
    delete (globalThis as any).__CCRESDOC_APPEARANCE__;
    delete (globalThis as any).__zudoDocThemePacks;
    localStorage.clear();
    delete document.documentElement.dataset.theme;
    delete document.documentElement.dataset.themePack;
  });

  it("keeps preview payloads out of persistent origin storage", async () => {
    let appearanceListener: ((event: any) => void) | undefined;
    localStorage.setItem("zudo-doc-theme", "light");
    localStorage.setItem("zudo-doc-theme-pack", "paper");
    document.documentElement.dataset.themePack = "paper";
    (globalThis as any).__CCRESDOC_APPEARANCE__ = {
      mode: "light", themePack: "paper", effectiveMode: "light",
      revision: "sha256:one", source: "authoritative", origin: location.origin,
    };
    (globalThis as any).__zudoDocThemePacks = { base: "/", packs: { default: "1", paper: "2" } };
    (globalThis as any).__TAURI__ = {
      core: { invoke: vi.fn() },
      event: { listen: vi.fn(async (_name, handler) => { appearanceListener = handler; return () => {}; }) },
    };
    const mount = document.createElement("div");
    document.body.append(mount);
    act(() => render(<AppearanceBridge />, mount));
    await flush();

    appearanceListener?.({ payload: {
      appearance: { mode: "dark", themePack: "default" },
      authoritative: { mode: "light", themePack: "paper" },
      revision: "sha256:one", source: "preview", authoritativeSource: "authoritative",
    } });
    await flush();

    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(localStorage.getItem("zudo-doc-theme")).toBe("light");
    expect(localStorage.getItem("zudo-doc-theme-pack")).toBe("paper");
    act(() => render(null, mount));
    mount.remove();
  });

  it("serializes a quick toggle once and suppresses authoritative echo writes", async () => {
    let appearanceListener: ((event: any) => void) | undefined;
    let resolveMutation!: (value: unknown) => void;
    const invoke = vi.fn(() => new Promise((resolve) => { resolveMutation = resolve; }));
    (globalThis as any).__CCRESDOC_APPEARANCE__ = {
      mode: "system", themePack: "default", effectiveMode: "light",
      revision: "sha256:one", source: "authoritative", origin: location.origin,
    };
    (globalThis as any).__zudoDocThemePacks = { base: "/", packs: { default: "1" } };
    (globalThis as any).__TAURI__ = {
      core: { invoke },
      event: { listen: vi.fn(async (_name, handler) => { appearanceListener = handler; return () => {}; }) },
    };
    const mount = document.createElement("div");
    document.body.append(mount);
    act(() => render(<AppearanceBridge />, mount));
    await flush();

    document.documentElement.dataset.theme = "dark";
    window.dispatchEvent(new CustomEvent("color-scheme-changed"));
    window.dispatchEvent(new CustomEvent("color-scheme-changed"));
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("update_appearance", {
      request: { mode: "dark", themePack: "default", intent: "persist", field: "mode" },
    });

    const authoritative = {
      appearance: { mode: "dark", themePack: "default" },
      authoritative: { mode: "dark", themePack: "default" },
      revision: "sha256:two", source: "authoritative", authoritativeSource: "authoritative",
    };
    resolveMutation(authoritative);
    await flush();
    appearanceListener?.({ payload: authoritative });
    await flush();
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(localStorage.getItem("ccresdoc-appearance-v1")).toContain("sha256:two");
  });

  it("reports a valid prepaint legacy candidate without persisting it", async () => {
    const invoke = vi.fn(async () => ({ appearance: { mode: "light", themePack: "default" } }));
    (globalThis as any).__CCRESDOC_APPEARANCE__ = {
      mode: "light", themePack: "default", effectiveMode: "light",
      revision: null, source: "legacy_candidate", origin: "http://localhost:4892",
    };
    (globalThis as any).__zudoDocThemePacks = { base: "/", packs: { default: "1" } };
    (globalThis as any).__TAURI__ = { core: { invoke }, event: { listen: async () => () => {} } };
    const mount = document.createElement("div"); document.body.append(mount);
    act(() => render(<AppearanceBridge />, mount)); await flush();
    expect(invoke).toHaveBeenCalledWith("update_appearance", {
      request: { mode: "light", themePack: "default", intent: "legacy_candidate", field: "mode" },
    });
    expect(localStorage.getItem("ccresdoc-appearance-v1")).toBeNull();
  });
});
