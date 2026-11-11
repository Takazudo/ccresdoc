import { createIslandTest } from "@takazudo/zfb/zudo-react/testing";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  APPEARANCE_CACHE_KEY,
  AppearanceBridge,
  STYLESHEET_TIMEOUT_MS,
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
      request: { mode: "dark", themePack: "classic", field: "mode" },
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
      request: { mode: "light", themePack: "graphite", field: "theme_pack" },
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

  it("restores zudo-doc's own state when a change is rejected before any config exists", async () => {
    const invoke = vi.fn(async () => { throw new Error("rejected"); });
    const { root } = fakeRoot({ invoke, boot: { mode: "system", source: "default", revision: null } });
    const scheme = root.__zudoDocColorScheme!;
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      start(root);
      // zudo-doc's toggle stores the choice before the bridge hears about it.
      scheme.choice = "dark";
      localStorage.setItem("zudo-doc-theme", "dark");
      window.dispatchEvent(new CustomEvent("theme-preference-changed"));
      await tick();
      await tick();
      expect(invoke).toHaveBeenCalledTimes(1);
      expect(scheme.choice).toBe("system");
      expect(localStorage.getItem("zudo-doc-theme")).toBe("system");
      expect(error).toHaveBeenCalled();
    } finally {
      error.mockRestore();
    }
  });

  it("calls the resolved unlisten exactly once on dispose", async () => {
    const { root, unlisten } = fakeRoot();
    const dispose = start(root);
    await tick();
    dispose();
    dispose();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("makes no writes when a stylesheet settles after dispose", async () => {
    document.documentElement.setAttribute("data-theme-pack", "classic");
    const { root, emit } = fakeRoot();
    const dispose = start(root);
    await tick();
    emit(envelope({ mode: "light", themePack: "graphite" }, "authoritative", "sha256:two"));
    await tick();
    const [loading] = packLinks("loading");
    const setItem = vi.spyOn(Storage.prototype, "setItem");
    const events = vi.fn();
    window.addEventListener("theme-pack-changed", events);
    try {
      dispose();
      expect(loading!.isConnected).toBe(false);
      settleLink(loading!, "load");
      await tick();
      expect(document.documentElement.getAttribute("data-theme-pack")).toBe("classic");
      expect(packLinks("active")).toEqual([]);
      expect(setItem).not.toHaveBeenCalled();
      expect(events).not.toHaveBeenCalled();
    } finally {
      setItem.mockRestore();
      window.removeEventListener("theme-pack-changed", events);
    }
  });

  it("ignores a stale stylesheet failure after a newer apply", async () => {
    document.documentElement.setAttribute("data-theme-pack", "classic");
    const { root, emit } = fakeRoot();
    start(root);
    await tick();
    emit(envelope({ mode: "light", themePack: "graphite" }, "authoritative", "sha256:two"));
    await tick();
    const [stale] = packLinks("loading");
    emit(envelope({ mode: "light", themePack: "academia" }, "authoritative", "sha256:three"));
    await tick();
    const fresh = packLinks("loading").find((link) => link !== stale)!;
    settleLink(stale!, "error");
    await tick();
    expect(stale!.isConnected).toBe(false);
    expect(document.documentElement.getAttribute("data-theme-pack")).toBe("classic");
    settleLink(fresh, "load");
    await tick();
    expect(document.documentElement.getAttribute("data-theme-pack")).toBe("academia");
    expect(packLinks("active")).toEqual([fresh]);
    expect(localStorage.getItem(APPEARANCE_CACHE_KEY)).toContain("sha256:three");
  });

  it("drops a stylesheet that times out and keeps the previous pack", async () => {
    vi.useFakeTimers();
    try {
      document.documentElement.setAttribute("data-theme-pack", "classic");
      const { root, emit } = fakeRoot();
      start(root);
      await vi.advanceTimersByTimeAsync(0);
      emit(envelope({ mode: "light", themePack: "graphite" }, "authoritative", "sha256:two"));
      await vi.advanceTimersByTimeAsync(0);
      expect(packLinks("loading")).toHaveLength(1);
      await vi.advanceTimersByTimeAsync(STYLESHEET_TIMEOUT_MS);
      expect(packLinks("loading")).toEqual([]);
      expect(document.documentElement.getAttribute("data-theme-pack")).toBe("classic");
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });

  it("coalesces rapid user changes to the first and the latest request", async () => {
    let release!: (value: AppearanceEnvelope) => void;
    const invoke = vi.fn((_command: string, args?: unknown) => {
      const request = (args as { request: { mode: "light"; themePack: string } }).request;
      if (invoke.mock.calls.length === 1) return new Promise<AppearanceEnvelope>((resolve) => { release = resolve; });
      return Promise.resolve(envelope({ mode: request.mode, themePack: request.themePack }, "authoritative", "sha256:last"));
    });
    const { root } = fakeRoot({ invoke });
    start(root);
    for (const pack of ["graphite", "academia", "graphite", "academia"]) {
      window.dispatchEvent(new CustomEvent("theme-pack-changed", { detail: { pack } }));
    }
    expect(invoke).toHaveBeenCalledTimes(1);
    release(envelope({ mode: "light", themePack: "graphite" }, "authoritative", "sha256:two"));
    await tick();
    settleLink(packLinks("loading")[0]!, "load");
    await tick();
    await tick();
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(invoke.mock.calls[1]![1]).toEqual({ request: { mode: "light", themePack: "academia", field: "theme_pack" } });
  });

  it("adds no duplicate listeners across a remount", async () => {
    const invoke = vi.fn(() => new Promise(() => {}));
    const listen = vi.fn(async () => vi.fn());
    const { root } = fakeRoot({ invoke, listen });
    start(root)();
    start(root);
    await tick();
    window.dispatchEvent(new CustomEvent("theme-pack-changed", { detail: { pack: "graphite" } }));
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(listen).toHaveBeenCalledTimes(2);
    const [first, second] = await Promise.all(listen.mock.results.map((result) => result.value));
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).not.toHaveBeenCalled();
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
