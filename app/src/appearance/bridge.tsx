"use client";

import { getScope } from "@takazudo/zfb/zudo-react";
import themePackCatalog from "@takazudo/zudo-doc/catalog";

export type AppearanceMode = "system" | "light" | "dark";
export type Appearance = { mode: AppearanceMode; themePack: string };
export type AppearanceSource = "authoritative" | "preview" | "default";
export type AppearanceEnvelope = {
  appearance: Appearance;
  authoritative: Appearance;
  revision: unknown;
  source: AppearanceSource;
  authoritativeSource: "authoritative" | "default";
};
type Bootstrap = Appearance & {
  effectiveMode: "light" | "dark";
  revision: unknown;
  source: "authoritative" | "default";
  origin: string;
};
type Unlisten = () => void;
export type AppearanceRoot = typeof globalThis & {
  __CCRESDOC_APPEARANCE__?: Bootstrap;
  __zudoDocThemePacks?: { base: string; packs: Record<string, string> };
  __zudoDocColorScheme?: { choice: string | null; lastMode?: string };
  __TAURI__?: {
    core?: { invoke?: (command: string, args?: unknown) => Promise<unknown> };
    event?: { listen?: (event: string, handler: (event: { payload: AppearanceEnvelope }) => void) => Promise<Unlisten> };
  };
};

export const APPEARANCE_EVENT = "ccresdoc://appearance";
/** Versioned cache written by the document-start bootstrap and this bridge. */
export const APPEARANCE_CACHE_KEY = "ccresdoc-appearance-v2";
export const DEFAULT_THEME_PACK = "classic";
export const STYLESHEET_TIMEOUT_MS = 10_000;
const MODE_STORAGE_KEY = "zudo-doc-theme";
const PACK_STORAGE_KEY = "zudo-doc-theme-pack";

const stylesheetPacks = new Set(
  themePackCatalog.packs.filter((pack) => pack.hasStylesheet).map((pack) => pack.slug),
);

function resolveMode(mode: AppearanceMode): "light" | "dark" {
  return mode === "system"
    ? window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light"
    : mode;
}

function safeStorage(action: () => void) {
  try {
    action();
  } catch {
    // Storage can be unavailable; the document appearance still applies.
  }
}

/**
 * Keeps the docs page's appearance in step with the host's authoritative
 * settings. Preview is display-only; only authoritative values reach storage.
 * Returns a dispose function that removes every listener and makes any
 * in-flight subscription or stylesheet load inert.
 */
export function startAppearanceBridge(root: AppearanceRoot = globalThis as AppearanceRoot): () => void {
  const invoke = root.__TAURI__?.core?.invoke;
  const bootAppearance = root.__CCRESDOC_APPEARANCE__;
  let active: Appearance = bootAppearance
    ? { mode: bootAppearance.mode, themePack: bootAppearance.themePack }
    : { mode: "system", themePack: DEFAULT_THEME_PACK };
  let authoritative: Appearance = { ...active };
  let authoritativeSource: "authoritative" | "default" = bootAppearance?.source ?? "default";
  let authoritativeRevision: unknown = bootAppearance?.revision ?? null;
  let applying = false;
  let pending = "";
  let queued: { appearance: Appearance; field: "mode" | "theme_pack" } | null = null;
  let applySequence = 0;
  let disposed = false;
  let unlisten: Unlisten | undefined;
  const pendingLoads = new Set<{ link: HTMLLinkElement; timer: ReturnType<typeof setTimeout> }>();

  const key = (value: Appearance) => `${value.mode}\n${value.themePack}`;
  const isCurrent = (token: number) => !disposed && token === applySequence;

  async function loadStylesheet(href: string): Promise<HTMLLinkElement | null> {
    const link = document.createElement("link");
    link.rel = "stylesheet";
    link.setAttribute("data-zd-theme-pack-css-loading", "");
    link.href = href;
    let entry!: { link: HTMLLinkElement; timer: ReturnType<typeof setTimeout> };
    const loaded = await new Promise<boolean>((resolve) => {
      entry = { link, timer: setTimeout(() => resolve(false), STYLESHEET_TIMEOUT_MS) };
      pendingLoads.add(entry);
      link.addEventListener("load", () => resolve(true), { once: true });
      link.addEventListener("error", () => resolve(false), { once: true });
      document.head.append(link);
    });
    clearTimeout(entry.timer);
    pendingLoads.delete(entry);
    return loaded ? link : (link.remove(), null);
  }

  async function applyPack(themePack: string, persist: boolean, token: number): Promise<boolean> {
    const runtime = root.__zudoDocThemePacks;
    if (!runtime || !Object.prototype.hasOwnProperty.call(runtime.packs, themePack)) return false;
    const previous = document.documentElement.getAttribute("data-theme-pack") || DEFAULT_THEME_PACK;
    if (previous === themePack) return true;
    let nextLink: HTMLLinkElement | null = null;
    if (stylesheetPacks.has(themePack)) {
      nextLink = await loadStylesheet(`${runtime.base}theme-packs/${themePack}/pack.css?v=${runtime.packs[themePack]}`);
      if (!nextLink) return false;
      if (!isCurrent(token)) {
        nextLink.remove();
        return false;
      }
    }
    if (!isCurrent(token)) return false;
    // The stylesheet is in place before the root attribute changes, so a
    // failed or superseded load always leaves the previous pack active.
    document.documentElement.setAttribute("data-theme-pack", themePack);
    for (const link of document.querySelectorAll<HTMLLinkElement>("link[data-zd-theme-pack-css],link[data-zd-theme-pack-css-loading]")) {
      if (link !== nextLink && ![...pendingLoads].some((entry) => entry.link === link)) link.remove();
    }
    if (nextLink) {
      nextLink.removeAttribute("data-zd-theme-pack-css-loading");
      nextLink.setAttribute("data-zd-theme-pack-css", "");
    }
    if (persist) safeStorage(() => localStorage.setItem(PACK_STORAGE_KEY, themePack));
    window.dispatchEvent(new CustomEvent("theme-pack-changed", { detail: { pack: themePack, previous, source: "ccresdoc" } }));
    return true;
  }

  async function apply(value: Appearance, source: AppearanceSource | "media") {
    if (disposed) return;
    const token = ++applySequence;
    active = { ...value };
    const persist = source === "authoritative";
    const mode = resolveMode(value.mode);
    applying = true;
    try {
      document.documentElement.dataset.theme = mode;
      document.documentElement.style.colorScheme = mode;
      if (persist) {
        safeStorage(() => localStorage.setItem(MODE_STORAGE_KEY, value.mode));
        const scheme = root.__zudoDocColorScheme;
        if (scheme) {
          scheme.choice = value.mode;
          scheme.lastMode = mode;
        }
        window.dispatchEvent(new CustomEvent("theme-preference-changed", { detail: { source: "ccresdoc" } }));
      }
      window.dispatchEvent(new CustomEvent("color-scheme-changed", { detail: { source: "ccresdoc" } }));
    } finally {
      applying = false;
    }
    const packApplied = await applyPack(value.themePack, persist, token);
    if (!isCurrent(token)) return;
    if (persist && packApplied) {
      safeStorage(() => localStorage.setItem(APPEARANCE_CACHE_KEY, JSON.stringify({ ...value, revision: authoritativeRevision })));
    }
  }

  async function accept(envelope: AppearanceEnvelope) {
    if (disposed) return;
    authoritative = { ...envelope.authoritative };
    authoritativeSource = envelope.authoritativeSource;
    authoritativeRevision = envelope.revision;
    pending = "";
    await apply(envelope.appearance, envelope.source);
  }

  async function persist(value: Appearance, field: "mode" | "theme_pack") {
    if (!invoke || disposed) return;
    if (pending) {
      if (pending !== key(value)) queued = { appearance: { ...value }, field };
      return;
    }
    if (key(value) === key(authoritative)) return;
    pending = key(value);
    try {
      const result = await invoke("update_appearance", { request: { ...value, field } }) as AppearanceEnvelope;
      await accept(result);
    } catch (error) {
      pending = "";
      await apply(authoritative, authoritativeSource);
      console.error("update_appearance failed:", error);
    } finally {
      const next = queued;
      queued = null;
      if (next && !disposed) void persist(next.appearance, next.field);
    }
  }

  const preferenceChanged = () => {
    if (applying) return;
    const choice = root.__zudoDocColorScheme?.choice;
    if (choice === "light" || choice === "dark" || choice === "system") void persist({ ...active, mode: choice }, "mode");
  };
  const packChanged = (event: Event) => {
    if (applying) return;
    const detail = (event as CustomEvent<{ pack?: unknown; source?: unknown }>).detail;
    if (detail?.source === "ccresdoc") return;
    if (typeof detail?.pack === "string") void persist({ ...active, themePack: detail.pack }, "theme_pack");
  };
  const media = window.matchMedia("(prefers-color-scheme: dark)");
  const mediaChanged = () => {
    if (active.mode === "system") void apply(active, "media");
  };
  window.addEventListener("theme-preference-changed", preferenceChanged);
  window.addEventListener("theme-pack-changed", packChanged);
  media.addEventListener("change", mediaChanged);

  const listen = root.__TAURI__?.event?.listen;
  if (listen) {
    void listen(APPEARANCE_EVENT, (event) => { void accept(event.payload); }).then(
      (stop) => {
        // A subscription that resolves after disposal is released at once.
        if (disposed) stop();
        else unlisten = stop;
      },
      (error) => console.error("appearance subscription failed:", error),
    );
  }

  return () => {
    if (disposed) return;
    disposed = true;
    unlisten?.();
    unlisten = undefined;
    queued = null;
    for (const entry of pendingLoads) {
      clearTimeout(entry.timer);
      entry.link.remove();
    }
    pendingLoads.clear();
    window.removeEventListener("theme-preference-changed", preferenceChanged);
    window.removeEventListener("theme-pack-changed", packChanged);
    media.removeEventListener("change", mediaChanged);
  };
}

export function AppearanceBridge() {
  getScope().onActivate(() => startAppearanceBridge());
  return null;
}

AppearanceBridge.displayName = "AppearanceBridge";
