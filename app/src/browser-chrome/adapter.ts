"use client";

import { navigate } from "@takazudo/zfb-runtime";
import { openFindInPage } from "@takazudo/zudo-doc/find-in-page";
import { openSearch } from "@takazudo/zudo-doc/search-widget-script";
import catalog from "./command-catalog.json";
import { ManagedHistoryController } from "./history";
import type {
  BrowserBootstrap,
  BrowserCommandEnvelope,
  BrowserCommandId,
  BrowserToolbarAdapter,
  BrowserToolbarSnapshot,
  ShortcutEntry,
} from "./types";

const COMMAND_EVENT = "ccresdoc://browser-command";
const BOOTSTRAP_EVENT = "ccresdoc://browser-bootstrap";
const BROWSER_SCOPE_KEY = "ccresdoc:browser-history-scope:v1";
const EDITING_SELECTOR = [
  "input",
  "textarea",
  "select",
  "[contenteditable]:not([contenteditable='false'])",
  "[data-shortcut-capture]",
  "[data-search-dialog]",
  "[data-search-input]",
  "input[aria-label='Find in page']",
].join(",");
const NONINTERACTIVE_EDITING_ANCESTOR = [
  "dialog:not([open])",
  "[hidden]",
  "[inert]",
  "[aria-hidden='true']",
].join(",");
const SHIFTED_PRINTABLE_KEYS: Record<string, string> = {
  "!": "1", "@": "2", "#": "3", "$": "4", "%": "5", "^": "6", "&": "7", "*": "8",
  "(": "9", ")": "0", _: "-", "+": "=", "{": "[", "}": "]", "|": "\\", ":": ";",
  "\"": "'", "<": ",", ">": ".", "?": "/", "~": "`",
};

type Unlisten = () => void;
type TauriRoot = Window & {
  find?: (query: string) => boolean;
  __TAURI__?: {
    core?: { invoke?: <T>(command: string, args?: Record<string, unknown>) => Promise<T> };
    event?: {
      listen?: <T>(event: string, listener: (event: { payload: T }) => void) => Promise<Unlisten>;
    };
  };
};

const ALL_COMMANDS: BrowserCommandId[] = [
  "back", "forward", "home", "reload-documentation", "find-in-page",
  "search-documentation", "copy-page-path", "open-in-default-browser", "settings",
];

function stablePath(location: Location): string {
  try {
    const path = decodeURI(location.pathname);
    return path === "/" ? "/docs/" : path;
  } catch {
    return location.pathname === "/" ? "/docs/" : location.pathname;
  }
}

function defaultAvailability(mode: "browser" | "tauri"): Record<BrowserCommandId, boolean> {
  return Object.fromEntries(ALL_COMMANDS.map((command) => [
    command,
    mode === "browser"
      ? !["reload-documentation", "open-in-default-browser", "settings"].includes(command)
      : !["reload-documentation", "open-in-default-browser", "settings"].includes(command),
  ])) as Record<BrowserCommandId, boolean>;
}

function browserScope(storage: Storage): string {
  try {
    const existing = storage.getItem(BROWSER_SCOPE_KEY);
    if (existing) return existing;
    const created = typeof crypto?.randomUUID === "function"
      ? crypto.randomUUID()
      : `${Date.now()}-${Math.random()}`;
    storage.setItem(BROWSER_SCOPE_KEY, created);
    return created;
  } catch {
    return "ephemeral";
  }
}

function configuredBrowserShortcuts(): ShortcutEntry[] {
  return catalog.commands.map((command) => ({
    commandId: command.commandId as BrowserCommandId,
    bindings: [...command.defaultBindings],
  }));
}

function normalizedBinding(binding: string, macos: boolean): string {
  return binding
    .split("+")
    .map((part) => part.trim().toLowerCase())
    .map((part) => !macos && part === "ctrl" ? "mod" : part)
    .sort()
    .join("+");
}

function macOSPlatform(root: Window): boolean {
  const navigator = root.navigator as Navigator & { userAgentData?: { platform?: string } };
  return /Mac|iPhone|iPad|iPod/i.test(navigator.userAgentData?.platform ?? navigator.platform ?? "");
}

export function keyboardEventBinding(event: KeyboardEvent, macos: boolean): string {
  const parts: string[] = [];
  if (macos) {
    if (event.metaKey) parts.push("mod");
    if (event.ctrlKey) parts.push("ctrl");
  } else {
    if (event.ctrlKey) parts.push("mod");
    // The Windows/Super key is not the portable Mod key off macOS. Retain an
    // unconfigurable marker so it cannot accidentally trigger another binding.
    if (event.metaKey) parts.push("meta");
  }
  if (event.altKey) parts.push("alt");
  if (event.shiftKey) parts.push("shift");
  if (event.getModifierState?.("AltGraph")) parts.push("altgraph");
  const rawKey = event.shiftKey ? SHIFTED_PRINTABLE_KEYS[event.key] ?? event.key : event.key;
  parts.push((rawKey === " " ? "Space" : rawKey).toLowerCase());
  return parts.sort().join("+");
}

function editingTarget(event: KeyboardEvent): boolean {
  const target = event.target instanceof Element ? event.target : null;
  const active = document.activeElement instanceof Element ? document.activeElement : null;
  const isInteractiveEditor = (element: Element | null) => {
    const editor = element?.closest(EDITING_SELECTOR);
    return Boolean(editor && !editor.closest(NONINTERACTIVE_EDITING_ANCESTOR));
  };
  return isInteractiveEditor(target) || isInteractiveEditor(active);
}

export class CCResDocBrowserAdapter implements BrowserToolbarAdapter {
  private listeners = new Set<(snapshot: BrowserToolbarSnapshot) => void>();
  private historyController: ManagedHistoryController | undefined;
  private shortcutEntries: ShortcutEntry[] = [];
  private nativeOwned = new Set<string>();
  private seenNativeInvocations = new Set<string>();
  private pendingNavigationType: "push" | "replace" | "traverse" | undefined;
  private started = false;
  private stopCallbacks: Unlisten[] = [];
  private snapshot: BrowserToolbarSnapshot;
  private readonly macos: boolean;

  constructor(private readonly root: TauriRoot = window as TauriRoot) {
    this.macos = macOSPlatform(root);
    const mode = root.__TAURI__?.core?.invoke ? "tauri" : "browser";
    this.snapshot = {
      title: document.title,
      stablePath: stablePath(root.location),
      canGoBack: false,
      canGoForward: false,
      traversalPending: false,
      mode,
      bootstrap: mode === "browser" ? "ready" : "loading",
      availability: defaultAvailability(mode),
    };
  }

  getSnapshot(): BrowserToolbarSnapshot {
    return this.snapshot;
  }

  subscribe(listener: (snapshot: BrowserToolbarSnapshot) => void): Unlisten {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  start(): Unlisten {
    if (this.started) return () => undefined;
    this.started = true;
    const on = <K extends keyof WindowEventMap>(target: Window | Document, type: K | string, listener: EventListener) => {
      target.addEventListener(type, listener);
      this.stopCallbacks.push(() => target.removeEventListener(type, listener));
    };

    on(document, "zfb:before-preparation", ((event: Event & { navigationType?: "push" | "replace" | "traverse" }) => {
      this.pendingNavigationType = event.navigationType;
    }) as EventListener);
    on(document, "zfb:page-load", (() => this.settleNavigation()) as EventListener);
    on(document, "zfb:navigation-aborted", (() => {
      this.historyController?.cancelTraversal();
      this.refresh();
    }) as EventListener);
    on(this.root, "popstate", ((event: PopStateEvent) => {
      this.pendingNavigationType = "traverse";
      this.historyController?.onPopState(event.state);
      this.refresh(false);
    }) as EventListener);
    on(this.root, "pageshow", ((event: PageTransitionEvent) => {
      if (event.persisted) {
        this.historyController?.onPopState(this.root.history.state);
        this.historyController?.settleSuccessfulNavigation("traverse");
        this.refresh();
      }
    }) as EventListener);
    on(document, "keydown", this.handleKeyDown as EventListener);

    if (this.snapshot.mode === "browser") {
      this.shortcutEntries = configuredBrowserShortcuts();
      this.establishHistory(`browser:${browserScope(this.root.sessionStorage)}`);
    } else {
      void this.startTauri();
    }
    this.refresh();

    return () => {
      for (const stop of this.stopCallbacks.splice(0)) stop();
      this.started = false;
    };
  }

  async execute(envelope: BrowserCommandEnvelope): Promise<boolean> {
    const command = envelope.commandId;
    if (!this.snapshot.availability[command]) return false;
    if (envelope.runtimeGeneration !== undefined
      && this.runtimeGeneration !== undefined
      && envelope.runtimeGeneration !== this.runtimeGeneration) return false;
    if (envelope.hostHandled) return true;

    switch (command) {
      case "back":
        if (!this.historyController?.traverse(-1)) return false;
        this.refresh();
        return true;
      case "forward":
        if (!this.historyController?.traverse(1)) return false;
        this.refresh();
        return true;
      case "home":
        await navigate("/docs/");
        return true;
      case "reload-documentation":
        return this.invokeHost("reload_documentation");
      case "open-in-default-browser":
        return this.invokeHost("open_current_page_in_default_browser");
      case "settings":
        return this.invokeHost("open_settings_window");
      case "copy-page-path":
        await this.copyPath();
        return true;
      case "search-documentation": {
        if (!document.querySelector("site-search")) return false;
        openSearch({ refresh: true });
        return true;
      }
      case "find-in-page":
        if (this.snapshot.mode === "browser") {
          const query = this.root.prompt("Find in page");
          if (!query) return false;
          return typeof this.root.find === "function" ? this.root.find(query) : false;
        }
        openFindInPage();
        return true;
    }
  }

  private runtimeGeneration: number | undefined;

  private async startTauri(): Promise<void> {
    const eventApi = this.root.__TAURI__?.event;
    const invoke = this.root.__TAURI__?.core?.invoke;
    try {
      if (eventApi?.listen) {
        const [stopCommand, stopBootstrap] = await Promise.all([
          eventApi.listen<BrowserCommandEnvelope>(COMMAND_EVENT, ({ payload }) => this.acceptNative(payload)),
          eventApi.listen<BrowserBootstrap>(BOOTSTRAP_EVENT, ({ payload }) => this.applyBootstrap(payload)),
        ]);
        this.stopCallbacks.push(stopCommand, stopBootstrap);
      }
      if (!invoke) throw new Error("Tauri invoke is unavailable");
      const bootstrap = await invoke<BrowserBootstrap>("get_browser_bootstrap");
      this.applyBootstrap(bootstrap);
    } catch (error) {
      console.error("browser bootstrap failed:", error);
      this.snapshot = { ...this.snapshot, bootstrap: "unavailable" };
      this.emit();
    }
  }

  private applyBootstrap(bootstrap: BrowserBootstrap): void {
    if (!Number.isInteger(bootstrap.runtimeGeneration)) return;
    const generationChanged = bootstrap.runtimeGeneration !== this.runtimeGeneration;
    if (generationChanged) this.seenNativeInvocations.clear();
    this.runtimeGeneration = bootstrap.runtimeGeneration;
    this.shortcutEntries = bootstrap.shortcutEntries;
    this.nativeOwned = new Set(
      bootstrap.nativeOwnedBindings.map((item) => normalizedBinding(item.binding, this.macos)),
    );
    const availability = { ...defaultAvailability("tauri") };
    availability["reload-documentation"] = bootstrap.hostCapabilities.reloadDocumentation;
    availability["open-in-default-browser"] = bootstrap.hostCapabilities.openInDefaultBrowser;
    availability.settings = true;
    this.snapshot = { ...this.snapshot, bootstrap: "ready", availability };
    if (generationChanged) this.establishHistory(`tauri:${bootstrap.runtimeGeneration}`);
    this.refresh();
  }

  private acceptNative(envelope: BrowserCommandEnvelope): void {
    if (envelope.runtimeGeneration !== this.runtimeGeneration) return;
    const invocation = String(envelope.invocationId);
    if (this.seenNativeInvocations.has(invocation)) return;
    this.seenNativeInvocations.add(invocation);
    if (this.seenNativeInvocations.size > 100) {
      const oldest = this.seenNativeInvocations.values().next().value;
      if (oldest !== undefined) this.seenNativeInvocations.delete(oldest);
    }
    void this.execute({ ...envelope, origin: "native_menu" }).catch((error) => {
      console.error(`browser command ${envelope.commandId} failed:`, error);
    });
  }

  private establishHistory(scope: string): void {
    this.historyController = new ManagedHistoryController(scope, this.root);
  }

  private settleNavigation(): void {
    this.historyController?.settleSuccessfulNavigation(this.pendingNavigationType);
    this.pendingNavigationType = undefined;
    this.refresh();
  }

  private refresh(syncHost = true): void {
    const history = this.historyController?.snapshot();
    this.snapshot = {
      ...this.snapshot,
      title: document.title,
      stablePath: stablePath(this.root.location),
      canGoBack: history?.canGoBack ?? false,
      canGoForward: history?.canGoForward ?? false,
      traversalPending: history?.traversalPending ?? false,
    };
    this.emit();
    if (syncHost && this.snapshot.mode === "tauri" && this.snapshot.bootstrap === "ready") {
      void this.root.__TAURI__?.core?.invoke?.("update_browser_navigation_state", {
        update: {
          canGoBack: this.snapshot.canGoBack,
          canGoForward: this.snapshot.canGoForward,
          currentStablePath: this.snapshot.stablePath,
          runtimeGeneration: this.runtimeGeneration,
        },
      }).catch((error) => console.error("navigation state update failed:", error));
    }
  }

  private emit(): void {
    for (const listener of this.listeners) listener(this.snapshot);
  }

  private handleKeyDown = (event: KeyboardEvent): void => {
    if (this.snapshot.bootstrap !== "ready" || editingTarget(event) || event.defaultPrevented) return;
    const binding = keyboardEventBinding(event, this.macos);
    if (this.nativeOwned.has(binding)) return;
    const entry = this.shortcutEntries.find((candidate) =>
      candidate.bindings.some((item) => normalizedBinding(item, this.macos) === binding));
    if (!entry || !this.snapshot.availability[entry.commandId]) return;
    // Let the ordinary browser own its native Find UI. Toolbar activation has
    // a window.find fallback because browsers do not expose that UI directly.
    if (this.snapshot.mode === "browser" && entry.commandId === "find-in-page") return;
    event.preventDefault();
    event.stopImmediatePropagation();
    void this.execute({ commandId: entry.commandId, origin: "keyboard", invocationId: `key:${Date.now()}` })
      .catch((error) => console.error(`browser command ${entry.commandId} failed:`, error));
  };

  private async invokeHost(command: string): Promise<boolean> {
    const invoke = this.root.__TAURI__?.core?.invoke;
    if (!invoke) return false;
    await invoke(command);
    return true;
  }

  private async copyPath(): Promise<void> {
    if (this.root.navigator.clipboard?.writeText) {
      try {
        await this.root.navigator.clipboard.writeText(this.snapshot.stablePath);
        return;
      } catch {
        // Permission can be denied in an ordinary browser; fall through to
        // selection-based copying before surfacing a command failure.
      }
    }
    const area = this.root.document.createElement("textarea");
    area.value = this.snapshot.stablePath;
    area.style.position = "fixed";
    area.style.opacity = "0";
    this.root.document.body.append(area);
    area.select();
    let copied = false;
    try {
      copied = this.root.document.execCommand("copy");
    } finally {
      area.remove();
    }
    if (!copied) throw new Error("the documentation path could not be copied");
  }
}

let singleton: CCResDocBrowserAdapter | undefined;
export function getCCResDocBrowserAdapter(): CCResDocBrowserAdapter {
  return singleton ??= new CCResDocBrowserAdapter();
}
