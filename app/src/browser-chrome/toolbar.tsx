"use client";

import {
  computed,
  flush,
  getScope,
  signal,
  type Description,
  type ReadonlySignal,
  type Ref,
} from "@takazudo/zfb/zudo-react";
import { getCCResDocBrowserAdapter } from "./adapter";
import type { BrowserCommandId, BrowserToolbarAdapter, BrowserToolbarSnapshot } from "./types";

const MENU_COMMANDS: Array<{ command: BrowserCommandId; label: string }> = [
  { command: "search-documentation", label: "Search Documentation" },
  { command: "copy-page-path", label: "Copy Page Path" },
  { command: "open-in-default-browser", label: "Open in Default Browser" },
  { command: "settings", label: "Settings" },
];

export const INITIAL_SNAPSHOT: BrowserToolbarSnapshot = {
  title: "Documentation",
  stablePath: "/docs/",
  canGoBack: false,
  canGoForward: false,
  traversalPending: false,
  mode: "browser",
  bootstrap: "loading",
  availability: {
    back: true,
    forward: true,
    home: true,
    "reload-documentation": false,
    "find-in-page": true,
    "search-documentation": true,
    "copy-page-path": true,
    "open-in-default-browser": false,
    settings: false,
  },
};

const PATHS: Record<BrowserCommandId | "more", () => Description | Description[]> = {
  back: () => <path d="m15 18-6-6 6-6" />,
  forward: () => <path d="m9 18 6-6-6-6" />,
  "reload-documentation": () => [<path d="M20 12a8 8 0 1 1-2.34-5.66" />, <path d="M20 4v6h-6" />],
  home: () => [<path d="m3 11 9-8 9 8" />, <path d="M5 10v10h14V10" />, <path d="M9 20v-6h6v6" />],
  "find-in-page": () => [<circle cx="11" cy="11" r="7" />, <path d="m20 20-4-4" />],
  "search-documentation": () => <path d="M4 6h16M4 12h16M4 18h10" />,
  "copy-page-path": () => [<rect x="9" y="9" width="11" height="11" rx="2" />, <path d="M15 9V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h3" />],
  "open-in-default-browser": () => [<path d="M14 4h6v6" />, <path d="m20 4-9 9" />, <path d="M18 13v6a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h6" />],
  settings: () => [<circle cx="12" cy="12" r="3" />, <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l-2.8 2.8A1.7 1.7 0 0 0 15 19.4l-1 .6-.4 1H9.5l-.4-1-1-.6a1.7 1.7 0 0 0-1.9.3l-2.8-2.8a1.7 1.7 0 0 0 .3-1.9l-.6-1-1-.4V9.5l1-.4.6-1a1.7 1.7 0 0 0-.3-1.9l2.8-2.8a1.7 1.7 0 0 0 1.9.3l1-.6.4-1h4.1l.4 1 1 .6a1.7 1.7 0 0 0 1.9-.3l2.8 2.8a1.7 1.7 0 0 0-.3 1.9l.6 1 1 .4v4.1l-1 .4-.6 1Z" />],
  more: () => [<circle cx="5" cy="12" r="1" fill="currentColor" />, <circle cx="12" cy="12" r="1" fill="currentColor" />, <circle cx="19" cy="12" r="1" fill="currentColor" />],
};

function Icon({ name }: { name: BrowserCommandId | "more" }) {
  return <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">{PATHS[name]()}</svg>;
}

export function commandDisabled(snapshot: BrowserToolbarSnapshot, command: BrowserCommandId): boolean {
  if (!snapshot.availability[command]) return true;
  if (command === "back") return !snapshot.canGoBack;
  if (command === "forward") return !snapshot.canGoForward;
  return false;
}

type AdapterSource = { readonly current: BrowserToolbarAdapter | null };

function BrowserToolbarMarkup({ snapshot, adapter }: {
  snapshot: ReadonlySignal<BrowserToolbarSnapshot>;
  adapter: AdapterSource;
}) {
  const scope = getScope();
  const menuOpen = signal(false);
  const toolbarRef: Ref<HTMLElement> = { current: null };
  const triggerRef: Ref<HTMLButtonElement> = { current: null };
  const menuRef: Ref<HTMLDivElement> = { current: null };
  const disabled = (command: BrowserCommandId) => computed(() => commandDisabled(snapshot.value, command));
  const path = computed(() => snapshot.value.stablePath);
  const pathTitle = computed(() => `${snapshot.value.title} — ${snapshot.value.stablePath}`);
  const bootstrap = computed(() => snapshot.value.bootstrap);
  const menuHidden = computed(() => !menuOpen.value);
  const menuExpanded = computed(() => (menuOpen.value ? "true" : "false"));

  const menuItems = () => [...(menuRef.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [])];

  const closeMenu = (restoreFocus = false) => {
    menuOpen.value = false;
    if (restoreFocus) void flush().then(() => triggerRef.current?.focus());
  };

  scope.effect(() => {
    if (!menuOpen.value) return;
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && !toolbarRef.current?.contains(event.target)) closeMenu();
    };
    const beforeNavigate = () => closeMenu();
    // Observe outside presses in capture phase so an unrelated control cannot
    // strand the menu open by stopping pointer propagation in its own handler.
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("zfb:before-preparation", beforeNavigate);
    return () => {
      document.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("zfb:before-preparation", beforeNavigate);
    };
  });

  const activate = (command: BrowserCommandId, origin: "toolbar" | "overflow") => {
    const current = adapter.current;
    if (!current || commandDisabled(snapshot.value, command)) return;
    void current.execute({ commandId: command, origin, invocationId: `${origin}:${Date.now()}` })
      .catch((error) => console.error(`browser command ${command} failed:`, error));
    if (origin === "overflow") closeMenu(true);
  };

  const onClick = (event: MouseEvent) => {
    const target = event.target instanceof Element ? event.target.closest<HTMLElement>("[data-browser-command]") : null;
    const command = target?.dataset.browserCommand as BrowserCommandId | "more" | undefined;
    if (!command) return;
    if (command === "more") {
      menuOpen.value = !menuOpen.value;
      return;
    }
    activate(command, target?.closest("[role='menu']") ? "overflow" : "toolbar");
  };

  const focusMenuItem = (direction: 1 | -1, from?: Element | null) => {
    const items = menuItems();
    if (!items.length) return;
    if (!from) {
      items[direction > 0 ? 0 : items.length - 1]?.focus();
      return;
    }
    const index = items.indexOf(from as HTMLButtonElement);
    items[(index + direction + items.length) % items.length]?.focus();
  };

  const onKeyDown = (event: KeyboardEvent) => {
    const command = event.target instanceof Element
      ? event.target.closest<HTMLElement>("[data-browser-command]")?.dataset.browserCommand
      : undefined;
    if (command === "more" && ["ArrowDown", "ArrowUp"].includes(event.key)) {
      event.preventDefault();
      menuOpen.value = true;
      const direction = event.key === "ArrowDown" ? 1 : -1;
      void flush().then(() => focusMenuItem(direction));
    } else if (menuOpen.value && event.key === "Escape") {
      event.preventDefault();
      closeMenu(true);
    } else if (menuOpen.value && ["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
      event.preventDefault();
      if (event.key === "Home" || event.key === "End") {
        const items = menuItems();
        items[event.key === "Home" ? 0 : items.length - 1]?.focus();
      } else {
        focusMenuItem(event.key === "ArrowDown" ? 1 : -1, event.target as Element);
      }
    } else if (menuOpen.value && event.key === "Tab") {
      closeMenu();
    }
  };

  const iconButton = (command: BrowserCommandId, label: string, extraClass = "") => (
    <button type="button" class={`ccresdoc-browser-toolbar__button ${extraClass}`.trim()} data-browser-command={command}
      title={label} aria-label={label} disabled={disabled(command)}>
      <Icon name={command} />
    </button>
  );

  return (
    <nav ref={toolbarRef} class="ccresdoc-browser-toolbar" aria-label="Browser navigation"
      data-ccresdoc-browser-toolbar-shell="" data-bootstrap={bootstrap} on:click={onClick} on:keydown={onKeyDown}>
      <div class="ccresdoc-browser-toolbar__leading">
        {iconButton("back", "Back")}
        {iconButton("forward", "Forward")}
        {iconButton("reload-documentation", "Reload Documentation")}
        {iconButton("home", "Home")}
      </div>
      <input class="ccresdoc-browser-toolbar__path" aria-label="Current documentation path" title={pathTitle}
        value={path} readonly={true} spellcheck={false} />
      <div class="ccresdoc-browser-toolbar__trailing">
        {iconButton("find-in-page", "Find in Page")}
        {iconButton("copy-page-path", "Copy Page Path", "ccresdoc-browser-toolbar__compactable")}
        {iconButton("open-in-default-browser", "Open in Default Browser", "ccresdoc-browser-toolbar__compactable")}
        <div class="ccresdoc-browser-toolbar__more">
          <button ref={triggerRef} type="button" class="ccresdoc-browser-toolbar__button" data-browser-command="more"
            title="More" aria-label="More browser actions" aria-haspopup="menu" aria-expanded={menuExpanded} aria-controls="ccresdoc-browser-more-menu">
            <Icon name="more" />
          </button>
          <div ref={menuRef} id="ccresdoc-browser-more-menu" class="ccresdoc-browser-toolbar__menu" role="menu" hidden={menuHidden}>
            {MENU_COMMANDS.map(({ command, label }) => (
              <button type="button" role="menuitem" data-browser-command={command} disabled={disabled(command)}>
                <Icon name={command} /><span>{label}</span>
              </button>
            ))}
          </div>
        </div>
      </div>
    </nav>
  );
}

function connectAdapter(
  adapter: BrowserToolbarAdapter,
  snapshot: { value: BrowserToolbarSnapshot },
): () => void {
  snapshot.value = adapter.getSnapshot();
  const unsubscribe = adapter.subscribe((next) => { snapshot.value = next; });
  const stop = adapter.start();
  return () => {
    unsubscribe();
    stop();
  };
}

/** Test seam: renders the toolbar against an explicit adapter. */
export function BrowserToolbarView({ adapter }: { adapter: BrowserToolbarAdapter }) {
  const snapshot = signal(adapter.getSnapshot());
  getScope().onActivate(() => connectAdapter(adapter, snapshot));
  return <BrowserToolbarMarkup snapshot={snapshot} adapter={{ current: adapter }} />;
}

export function CCResDocBrowserToolbar() {
  const snapshot = signal(INITIAL_SNAPSHOT);
  const adapter: { current: BrowserToolbarAdapter | null } = { current: null };
  getScope().onActivate(() => {
    const current = getCCResDocBrowserAdapter();
    adapter.current = current;
    const disconnect = connectAdapter(current, snapshot);
    return () => {
      disconnect();
      adapter.current = null;
    };
  });
  return <BrowserToolbarMarkup snapshot={snapshot} adapter={adapter} />;
}

CCResDocBrowserToolbar.displayName = "CCResDocBrowserToolbar";
