"use client";

import { h, type JSX } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import { getCCResDocBrowserAdapter } from "./adapter";
import type { BrowserCommandId, BrowserToolbarAdapter, BrowserToolbarSnapshot } from "./types";

const MENU_COMMANDS: Array<{ command: BrowserCommandId; label: string }> = [
  { command: "search-documentation", label: "Search Documentation" },
  { command: "settings", label: "Settings" },
  { command: "copy-page-path", label: "Copy Page Path" },
  { command: "open-in-default-browser", label: "Open in Default Browser" },
];

const INITIAL_SNAPSHOT: BrowserToolbarSnapshot = {
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

const PATHS: Record<BrowserCommandId | "more", JSX.Element> = {
  back: <path d="m15 18-6-6 6-6" />,
  forward: <path d="m9 18 6-6-6-6" />,
  "reload-documentation": <><path d="M20 12a8 8 0 1 1-2.34-5.66" /><path d="M20 4v6h-6" /></>,
  home: <><path d="m3 11 9-8 9 8" /><path d="M5 10v10h14V10" /><path d="M9 20v-6h6v6" /></>,
  "find-in-page": <><circle cx="11" cy="11" r="7" /><path d="m20 20-4-4" /></>,
  "search-documentation": <path d="M4 6h16M4 12h16M4 18h10" />,
  "copy-page-path": <><rect x="9" y="9" width="11" height="11" rx="2" /><path d="M15 9V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h3" /></>,
  "open-in-default-browser": <><path d="M14 4h6v6" /><path d="m20 4-9 9" /><path d="M18 13v6a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h6" /></>,
  settings: <><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l-2.8 2.8A1.7 1.7 0 0 0 15 19.4l-1 .6-.4 1H9.5l-.4-1-1-.6a1.7 1.7 0 0 0-1.9.3l-2.8-2.8a1.7 1.7 0 0 0 .3-1.9l-.6-1-1-.4V9.5l1-.4.6-1a1.7 1.7 0 0 0-.3-1.9l2.8-2.8a1.7 1.7 0 0 0 1.9.3l1-.6.4-1h4.1l.4 1 1 .6a1.7 1.7 0 0 0 1.9-.3l2.8 2.8a1.7 1.7 0 0 0-.3 1.9l.6 1 1 .4v4.1l-1 .4-.6 1Z" /></>,
  more: <><circle cx="5" cy="12" r="1" fill="currentColor" /><circle cx="12" cy="12" r="1" fill="currentColor" /><circle cx="19" cy="12" r="1" fill="currentColor" /></>,
};

function Icon({ name }: { name: BrowserCommandId | "more" }) {
  return <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">{PATHS[name]}</svg>;
}

function commandDisabled(snapshot: BrowserToolbarSnapshot, command: BrowserCommandId): boolean {
  if (!snapshot.availability[command]) return true;
  if (command === "back") return !snapshot.canGoBack;
  if (command === "forward") return !snapshot.canGoForward;
  return false;
}

export function BrowserToolbarView({ adapter }: { adapter: BrowserToolbarAdapter }) {
  const [snapshot, setSnapshot] = useState(adapter.getSnapshot());
  useEffect(() => adapter.subscribe(setSnapshot), [adapter]);
  useEffect(() => adapter.start(), [adapter]);
  return <BrowserToolbarMarkup snapshot={snapshot} adapter={adapter} />;
}

function BrowserToolbarMarkup({ snapshot, adapter }: {
  snapshot: BrowserToolbarSnapshot;
  adapter?: BrowserToolbarAdapter;
}) {
  const [menuOpen, setMenuOpen] = useState(false);
  const toolbarRef = useRef<HTMLElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  const closeMenu = (restoreFocus = false) => {
    setMenuOpen(false);
    if (restoreFocus) queueMicrotask(() => triggerRef.current?.focus());
  };

  useEffect(() => {
    if (!menuOpen) return;
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && !toolbarRef.current?.contains(event.target)) closeMenu();
    };
    const beforeNavigate = () => closeMenu();
    document.addEventListener("pointerdown", outside);
    document.addEventListener("zfb:before-preparation", beforeNavigate);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("zfb:before-preparation", beforeNavigate);
    };
  }, [menuOpen]);

  const activate = (command: BrowserCommandId, origin: "toolbar" | "overflow") => {
    if (!adapter || commandDisabled(snapshot, command)) return;
    void adapter.execute({ commandId: command, origin, invocationId: `${origin}:${Date.now()}` });
    if (origin === "overflow") closeMenu(true);
  };

  const onClick = (event: JSX.TargetedMouseEvent<HTMLElement>) => {
    const target = event.target instanceof Element ? event.target.closest<HTMLElement>("[data-browser-command]") : null;
    const command = target?.dataset.browserCommand as BrowserCommandId | "more" | undefined;
    if (!command) return;
    if (command === "more") {
      setMenuOpen((open) => !open);
      return;
    }
    activate(command, target?.closest("[role='menu']") ? "overflow" : "toolbar");
  };

  const focusMenuItem = (direction: 1 | -1, from?: Element | null) => {
    const items = [...(menuRef.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [])];
    if (!items.length) return;
    if (!from) {
      items[direction > 0 ? 0 : items.length - 1]?.focus();
      return;
    }
    const index = from ? items.indexOf(from as HTMLButtonElement) : -1;
    items[(index + direction + items.length) % items.length]?.focus();
  };

  const onKeyDown = (event: JSX.TargetedKeyboardEvent<HTMLElement>) => {
    const command = event.target instanceof Element
      ? event.target.closest<HTMLElement>("[data-browser-command]")?.dataset.browserCommand
      : undefined;
    if (command === "more" && ["ArrowDown", "ArrowUp"].includes(event.key)) {
      event.preventDefault();
      setMenuOpen(true);
      queueMicrotask(() => focusMenuItem(event.key === "ArrowDown" ? 1 : -1));
    } else if (menuOpen && event.key === "Escape") {
      event.preventDefault();
      closeMenu(true);
    } else if (menuOpen && ["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
      event.preventDefault();
      if (event.key === "Home" || event.key === "End") {
        const items = [...(menuRef.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [])];
        items[event.key === "Home" ? 0 : items.length - 1]?.focus();
      } else {
        focusMenuItem(event.key === "ArrowDown" ? 1 : -1, event.target as Element);
      }
    } else if (menuOpen && event.key === "Tab") {
      closeMenu();
    }
  };

  const iconButton = (command: BrowserCommandId, label: string, extraClass = "") => (
    <button type="button" class={`ccresdoc-browser-toolbar__button ${extraClass}`} data-browser-command={command}
      title={label} aria-label={label} disabled={commandDisabled(snapshot, command)}>
      <Icon name={command} />
    </button>
  );

  return (
    <nav ref={toolbarRef} class="ccresdoc-browser-toolbar" aria-label="Browser navigation"
      data-bootstrap={snapshot.bootstrap} onClick={onClick} onKeyDown={onKeyDown}>
      <div class="ccresdoc-browser-toolbar__leading">
        {iconButton("back", "Back")}
        {iconButton("forward", "Forward")}
        {iconButton("reload-documentation", "Reload Documentation")}
        {iconButton("home", "Home")}
      </div>
      <input class="ccresdoc-browser-toolbar__path" aria-label="Current documentation path" title={`${snapshot.title} — ${snapshot.stablePath}`}
        value={snapshot.stablePath} readOnly spellcheck={false} />
      <div class="ccresdoc-browser-toolbar__trailing">
        {iconButton("find-in-page", "Find in Page")}
        {iconButton("copy-page-path", "Copy Page Path", "ccresdoc-browser-toolbar__compactable")}
        {iconButton("open-in-default-browser", "Open in Default Browser", "ccresdoc-browser-toolbar__compactable")}
        <div class="ccresdoc-browser-toolbar__more">
          <button ref={triggerRef} type="button" class="ccresdoc-browser-toolbar__button" data-browser-command="more"
            title="More" aria-label="More browser actions" aria-haspopup="menu" aria-expanded={menuOpen} aria-controls="ccresdoc-browser-more-menu">
            <Icon name="more" />
          </button>
          <div ref={menuRef} id="ccresdoc-browser-more-menu" class="ccresdoc-browser-toolbar__menu" role="menu" hidden={!menuOpen}>
            {MENU_COMMANDS.map(({ command, label }) => (
              <button key={command} type="button" role="menuitem" data-browser-command={command} disabled={commandDisabled(snapshot, command)}>
                <Icon name={command} /><span>{label}</span>
              </button>
            ))}
          </div>
        </div>
      </div>
    </nav>
  );
}

export function CCResDocBrowserToolbar() {
  const [adapter, setAdapter] = useState<BrowserToolbarAdapter>();
  const [snapshot, setSnapshot] = useState(INITIAL_SNAPSHOT);
  useEffect(() => {
    const current = getCCResDocBrowserAdapter();
    setAdapter(current);
    setSnapshot(current.getSnapshot());
    const unsubscribe = current.subscribe(setSnapshot);
    const stop = current.start();
    return () => { unsubscribe(); stop(); };
  }, []);
  return <BrowserToolbarMarkup snapshot={snapshot} adapter={adapter} />;
}

CCResDocBrowserToolbar.displayName = "CCResDocBrowserToolbar";
