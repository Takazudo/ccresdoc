import { createIslandTest } from "@takazudo/zfb/zudo-react/testing";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  APPEARANCE_CACHE_KEY,
  AppearanceBridge,
  startAppearanceBridge,
  type AppearanceEnvelope,
  type AppearanceRoot,
} from "../src/appearance/bridge";

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
const PACKS = { base: "/", packs: { classic: "1", graphite: "2", academia: "3" } };
type Handler = (event: { payload: AppearanceEnvelope }) => void;

function envelope(
  appearance: AppearanceEnvelope["appearance"],
  source: AppearanceEnvelope["source"],
  revision: string,
  authoritative = appearance,
): AppearanceEnvelope {
  return { appearance, authoritative, revision, source, authoritativeSource: "authoritative" };
}

function fakeRoot(options: {
  boot?: Partial<NonNullable<AppearanceRoot["__CCRESDOC_APPEARANCE__"]>>;
  invoke?: (command: string, args?: unknown) => Promise<unknown>;
  listen?: (event: string, handler: Handler) => Promise<() => void>;
} = {}) {
  let handler: Handler | undefined;
  const unlisten = vi.fn();
  const listen = options.listen ?? vi.fn(async (_event: string, next: Handler) => {
    handler = next;
    return unlisten;
  });
  const root = {
    __CCRESDOC_APPEARANCE__: {
      mode: "light", themePack: "classic", effectiveMode: "light",
      revision: "sha256:one", source: "authoritative", origin: location.origin,
      ...options.boot,
    },
    __zudoDocThemePacks: PACKS,
    __zudoDocColorScheme: { choice: null as string | null },
    __TAURI__: { core: { invoke: options.invoke ?? vi.fn() }, event: { listen } },
  } as unknown as AppearanceRoot;
  return { root, unlisten, emit: (payload: AppearanceEnvelope) => handler?.({ payload }) };
}

function packLinks(kind: "loading" | "active") {
  const attribute = kind === "loading" ? "data-zd-theme-pack-css-loading" : "data-zd-theme-pack-css";
  return [...document.querySelectorAll<HTMLLinkElement>(`link[${attribute}]`)];
}

// happy-dom rejects external stylesheets synchronously; hold each load open so
// the test decides when it settles.
const settled = new WeakSet<Event>();
const holdStylesheetLoads = (event: Event) => {
  if (event.target instanceof HTMLLinkElement && !settled.has(event)) event.stopImmediatePropagation();
};
document.addEventListener("error", holdStylesheetLoads, true);
function settleLink(link: HTMLLinkElement, type: "load" | "error") {
  const event = new Event(type);
  settled.add(event);
  link.dispatchEvent(event);
}

const disposers: Array<() => void> = [];
function start(root: AppearanceRoot) {
  const dispose = startAppearanceBridge(root);
  disposers.push(dispose);
  return dispose;
}

afterEach(() => {
  for (const dispose of disposers.splice(0)) dispose();
  document.head.querySelectorAll("link").forEach((link) => link.remove());
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.removeAttribute("data-theme-pack");
  document.documentElement.style.colorScheme = "";
});

describe("docs appearance bridge", () => {
  it("keeps preview display-only and persists only authoritative appearance", async () => {
    localStorage.setItem("zudo-doc-theme", "light");
    localStorage.setItem("zudo-doc-theme-pack", "classic");
    const { root, emit } = fakeRoot();
    start(root);
    await tick();

    emit(envelope({ mode: "dark", themePack: "classic" }, "preview", "sha256:one", { mode: "light", themePack: "classic" }));
    await tick();
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(localStorage.getItem("zudo-doc-theme")).toBe("light");
    expect(localStorage.getItem(APPEARANCE_CACHE_KEY)).toBeNull();

    emit(envelope({ mode: "system", themePack: "classic" }, "authoritative", "sha256:two"));
    await tick();
    expect(localStorage.getItem("zudo-doc-theme")).toBe("system");
    expect(localStorage.getItem("zudo-doc-theme-pack")).toBe("classic");
    expect(JSON.parse(localStorage.getItem(APPEARANCE_CACHE_KEY)!)).toEqual({
      mode: "system", themePack: "classic", revision: "sha256:two",
    });
    expect((root as { __zudoDocColorScheme: { choice: string } }).__zudoDocColorScheme.choice).toBe("system");
  });

  it("serializes a quick toggle once and suppresses the authoritative echo", async () => {
    let resolveMutation!: (value: unknown) => void;
    const invoke = vi.fn(() => new Promise((resolve) => { resolveMutation = resolve; }));
    const { root, emit } = fakeRoot({ boot: { mode: "system" }, invoke });
    start(root);
    await tick();

    const scheme = (root as { __zudoDocColorScheme: { choice: string | null } }).__zudoDocColorScheme;
    scheme.choice = "dark";
    window.dispatchEvent(new CustomEvent("theme-preference-changed"));
    window.dispatchEvent(new CustomEvent("theme-preference-changed"));
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("update_appearance", {
      request: { mode: "dark", themePack: "classic", intent: "persist", field: "mode" },
    });

    const authoritative = envelope({ mode: "dark", themePack: "classic" }, "authoritative", "sha256:two");
    resolveMutation(authoritative);
    await tick();
    emit(authoritative);
    await tick();
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(localStorage.getItem(APPEARANCE_CACHE_KEY)).toContain("sha256:two");
  });

  it("ignores its own pack events and persists a user pack choice", async () => {
    const invoke = vi.fn(async () => envelope({ mode: "light", themePack: "classic" }, "authoritative", "sha256:x"));
    const { root } = fakeRoot({ invoke });
    start(root);
    window.dispatchEvent(new CustomEvent("theme-pack-changed", { detail: { pack: "graphite", source: "ccresdoc" } }));
    expect(invoke).not.toHaveBeenCalled();
    window.dispatchEvent(new CustomEvent("theme-pack-changed", { detail: { pack: "graphite" } }));
    expect(invoke).toHaveBeenCalledWith("update_appearance", {
      request: { mode: "light", themePack: "graphite", intent: "persist", field: "theme_pack" },
    });
  });

  it("switches packs only after the stylesheet loads and keeps the old pack on failure", async () => {
    document.documentElement.setAttribute("data-theme-pack", "classic");
    const { root, emit } = fakeRoot();
    start(root);
    await tick();

    emit(envelope({ mode: "light", themePack: "graphite" }, "authoritative", "sha256:two"));
    await tick();
    const [loading] = packLinks("loading");
    expect(loading?.getAttribute("href")).toBe("/theme-packs/graphite/pack.css?v=2");
    expect(document.documentElement.getAttribute("data-theme-pack")).toBe("classic");
    settleLink(loading!, "load");
    await tick();
    expect(document.documentElement.getAttribute("data-theme-pack")).toBe("graphite");
    expect(packLinks("active")).toEqual([loading]);
    expect(localStorage.getItem("zudo-doc-theme-pack")).toBe("graphite");

    emit(envelope({ mode: "light", themePack: "academia" }, "authoritative", "sha256:three"));
    await tick();
    settleLink(packLinks("loading")[0]!, "error");
    await tick();
    expect(document.documentElement.getAttribute("data-theme-pack")).toBe("graphite");
    expect(packLinks("loading")).toEqual([]);
    expect(packLinks("active")).toEqual([loading]);
    expect(localStorage.getItem("zudo-doc-theme-pack")).toBe("graphite");
    expect(localStorage.getItem(APPEARANCE_CACHE_KEY)).toContain("sha256:two");
  });

  it("discards a superseded stylesheet load", async () => {
    document.documentElement.setAttribute("data-theme-pack", "classic");
    const { root, emit } = fakeRoot();
    start(root);
    await tick();
    emit(envelope({ mode: "light", themePack: "graphite" }, "preview", "sha256:one"));
    await tick();
    const [stale] = packLinks("loading");
    emit(envelope({ mode: "light", themePack: "classic" }, "authoritative", "sha256:two"));
    await tick();
    settleLink(stale!, "load");
    await tick();
    expect(document.documentElement.getAttribute("data-theme-pack")).toBe("classic");
    expect(stale!.isConnected).toBe(false);
  });

  it("releases listeners, late subscriptions and pending loads on dispose", async () => {
    let resolveListen!: (stop: () => void) => void;
    const stop = vi.fn();
    const invoke = vi.fn();
    const { root } = fakeRoot({
      invoke,
      listen: () => new Promise((resolve) => { resolveListen = resolve; }),
    });
    const dispose = start(root);
    window.dispatchEvent(new CustomEvent("theme-pack-changed", { detail: { pack: "graphite" } }));
    expect(invoke).toHaveBeenCalledTimes(1);
    dispose();

    resolveListen(stop);
    await tick();
    expect(stop).toHaveBeenCalledTimes(1);
    window.dispatchEvent(new CustomEvent("theme-pack-changed", { detail: { pack: "academia" } }));
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("starts from the island activation and stops on teardown", async () => {
    const invoke = vi.fn(async () => envelope({ mode: "light", themePack: "classic" }, "authoritative", "sha256:x"));
    const globals = globalThis as AppearanceRoot;
    const { root } = fakeRoot({ invoke });
    Object.assign(globals, { __TAURI__: root.__TAURI__, __CCRESDOC_APPEARANCE__: root.__CCRESDOC_APPEARANCE__ });
    try {
      const island = createIslandTest(AppearanceBridge, {}, { document });
      expect(island.hydrate()).not.toBeNull();
      await island.flush();
      expect(island.diagnostics).toEqual([]);
      window.dispatchEvent(new CustomEvent("theme-pack-changed", { detail: { pack: "graphite" } }));
      expect(invoke).toHaveBeenCalledTimes(1);
      island.dispose();
      window.dispatchEvent(new CustomEvent("theme-pack-changed", { detail: { pack: "academia" } }));
      expect(invoke).toHaveBeenCalledTimes(1);
    } finally {
      delete globals.__TAURI__;
      delete globals.__CCRESDOC_APPEARANCE__;
    }
  });
});
