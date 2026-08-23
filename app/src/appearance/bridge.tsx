"use client";

import { useEffect } from "preact/hooks";

type Mode = "system" | "light" | "dark";
type Appearance = { mode: Mode; themePack: string };
type Envelope = {
  appearance: Appearance;
  authoritative: Appearance;
  revision: { 0?: string } | string | null;
  source: "authoritative" | "preview" | "legacy_candidate" | "default";
  authoritativeSource: "authoritative" | "default";
};
type Bootstrap = Appearance & {
  effectiveMode: "light" | "dark";
  revision: unknown;
  source: "authoritative" | "legacy_candidate" | "default";
  origin: string;
};
type TauriRoot = typeof globalThis & {
  __CCRESDOC_APPEARANCE__?: Bootstrap;
  __zudoDocThemePacks?: { base: string; packs: Record<string, string> };
  __TAURI__?: {
    core?: { invoke?: (command: string, args?: unknown) => Promise<unknown> };
    event?: { listen?: (event: string, handler: (event: { payload: Envelope }) => void) => Promise<() => void> };
  };
};

const EVENT = "ccresdoc://appearance";
const CACHE_KEY = "ccresdoc-appearance-v1";

function effectiveMode(mode: Mode): "light" | "dark" {
  return mode === "system"
    ? window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light"
    : mode;
}

export function AppearanceBridge() {
  useEffect(() => {
    const root = globalThis as TauriRoot;
    const invoke = root.__TAURI__?.core?.invoke;
    let applying = false;
    const bootAppearance = root.__CCRESDOC_APPEARANCE__;
    let active: Appearance = bootAppearance
      ? { mode: bootAppearance.mode, themePack: bootAppearance.themePack }
      : { mode: "system", themePack: "default" };
    let authoritative: Appearance = { mode: active.mode, themePack: active.themePack };
    let authoritativeSource: "authoritative" | "legacy_candidate" | "default" =
      bootAppearance?.source ?? "default";
    let authoritativeRevision: unknown = root.__CCRESDOC_APPEARANCE__?.revision ?? null;
    let pending = "";
    let queued: { appearance: Appearance; field: "mode" | "theme_pack" } | null = null;
    let applySequence = 0;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const originalStorage = {
      mode: localStorage.getItem("zudo-doc-theme"),
      themePack: localStorage.getItem("zudo-doc-theme-pack"),
    };

    const key = (value: Appearance) => `${value.mode}\n${value.themePack}`;

    function dispatchColor() {
      window.dispatchEvent(new CustomEvent("color-scheme-changed", { detail: { source: "ccresdoc" } }));
    }

    async function applyPack(themePack: string, writeStorage: boolean, isCurrent: () => boolean) {
      const runtime = root.__zudoDocThemePacks;
      if (!runtime || !Object.prototype.hasOwnProperty.call(runtime.packs, themePack)) return;
      const current = document.documentElement.getAttribute("data-theme-pack") ?? "default";
      if (current === themePack) return;
      let nextLink: HTMLLinkElement | null = null;
      if (themePack !== "default") {
        nextLink = document.createElement("link");
        nextLink.rel = "stylesheet";
        nextLink.setAttribute("data-zd-theme-pack-css-loading", "");
        nextLink.href = `${runtime.base}theme-packs/${themePack}/pack.css?v=${runtime.packs[themePack]}`;
        const loaded = new Promise<boolean>((resolve) => {
          nextLink!.addEventListener("load", () => resolve(true), { once: true });
          nextLink!.addEventListener("error", () => resolve(false), { once: true });
          window.setTimeout(() => resolve(false), 10_000);
        });
        document.head.append(nextLink);
        if (!await loaded || !isCurrent()) { nextLink.remove(); return; }
      }
      if (!isCurrent()) return;
      document.documentElement.setAttribute("data-theme-pack", themePack);
      for (const link of document.querySelectorAll<HTMLLinkElement>("link[data-zd-theme-pack-css],link[data-zd-theme-pack-css-loading]")) {
        if (link !== nextLink) link.remove();
      }
      if (nextLink) {
        nextLink.removeAttribute("data-zd-theme-pack-css-loading");
        nextLink.setAttribute("data-zd-theme-pack-css", "");
      }
      if (writeStorage) localStorage.setItem("zudo-doc-theme-pack", themePack);
      window.dispatchEvent(new CustomEvent("theme-pack-changed", { detail: { pack: themePack, previous: current, source: "ccresdoc" } }));
    }

    async function apply(value: Appearance, source: "authoritative" | "preview" | "legacy_candidate" | "default" | "media") {
      const token = ++applySequence;
      active = { ...value };
      applying = true;
      const mode = effectiveMode(value.mode);
      document.documentElement.dataset.theme = mode;
      document.documentElement.style.colorScheme = mode;
      const project = source === "authoritative" || source === "preview";
      if (project) {
        if (value.mode === "system") localStorage.removeItem("zudo-doc-theme");
        else localStorage.setItem("zudo-doc-theme", value.mode);
      }
      dispatchColor();
      await applyPack(value.themePack, project, () => token === applySequence);
      if (token !== applySequence) return;
      if (project) localStorage.setItem("zudo-doc-theme-pack", value.themePack);
      if (source === "legacy_candidate" || source === "default") {
        if (originalStorage.mode === null) localStorage.removeItem("zudo-doc-theme");
        else localStorage.setItem("zudo-doc-theme", originalStorage.mode);
        if (originalStorage.themePack === null) localStorage.removeItem("zudo-doc-theme-pack");
        else localStorage.setItem("zudo-doc-theme-pack", originalStorage.themePack);
      }
      if (source === "authoritative") localStorage.setItem(CACHE_KEY, JSON.stringify({ ...value, revision: authoritativeRevision }));
      applying = false;
    }

    async function accept(envelope: Envelope) {
      if (disposed) return;
      authoritative = { ...envelope.authoritative };
      authoritativeSource = envelope.authoritativeSource;
      authoritativeRevision = envelope.revision;
      pending = "";
      await apply(envelope.appearance, envelope.source);
    }

    async function persist(value: Appearance, field: "mode" | "theme_pack") {
      if (!invoke || applying) return;
      if (pending) {
        if (pending !== key(value)) queued = { appearance: { ...value }, field };
        return;
      }
      if (key(value) === key(authoritative)) return;
      pending = key(value);
      try {
        const result = await invoke("update_appearance", { request: { ...value, intent: "persist", field } }) as Envelope;
        await accept(result);
      } catch (error) {
        pending = "";
        await apply(authoritative, authoritativeSource);
        console.error("update_appearance failed:", error);
      } finally {
        const next = queued;
        queued = null;
        if (next) void persist(next.appearance, next.field);
      }
    }

    const colorChanged = () => {
      if (applying) return;
      const mode = document.documentElement.dataset.theme;
      if (mode === "light" || mode === "dark") void persist({ ...active, mode }, "mode");
    };
    const packChanged = (event: Event) => {
      if (applying) return;
      const pack = (event as CustomEvent<{ pack?: unknown }>).detail?.pack;
      if (typeof pack === "string") void persist({ ...active, themePack: pack }, "theme_pack");
    };
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const mediaChanged = () => { if (active.mode === "system") void apply(active, "media"); };
    window.addEventListener("color-scheme-changed", colorChanged);
    window.addEventListener("theme-pack-changed", packChanged);
    media.addEventListener("change", mediaChanged);

    const boot = root.__CCRESDOC_APPEARANCE__;
    if (boot?.source === "legacy_candidate" && invoke) {
      void invoke("update_appearance", {
        request: { mode: boot.mode, themePack: boot.themePack, intent: "legacy_candidate", field: "mode" },
      }).catch((error) => console.warn("legacy appearance candidate was not accepted:", error));
    }
    const listen = root.__TAURI__?.event?.listen;
    if (listen) void listen(EVENT, (event) => { void accept(event.payload); }).then((stop) => { unlisten = stop; });

    return () => {
      disposed = true;
      unlisten?.();
      window.removeEventListener("color-scheme-changed", colorChanged);
      window.removeEventListener("theme-pack-changed", packChanged);
      media.removeEventListener("change", mediaChanged);
    };
  }, []);
  return null;
}

AppearanceBridge.displayName = "AppearanceBridge";
