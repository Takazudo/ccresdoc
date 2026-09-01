export type BrowserCommandId =
  | "back"
  | "forward"
  | "home"
  | "reload-documentation"
  | "find-in-page"
  | "search-documentation"
  | "copy-page-path"
  | "open-in-default-browser"
  | "settings";
export type BrowserCommandOrigin = "toolbar" | "overflow" | "keyboard" | "native_menu";

export interface BrowserCommandEnvelope {
  commandId: BrowserCommandId;
  origin: BrowserCommandOrigin;
  invocationId: number | string;
  runtimeGeneration?: number;
  hostHandled?: boolean;
}

export interface BrowserToolbarSnapshot {
  title: string;
  stablePath: string;
  canGoBack: boolean;
  canGoForward: boolean;
  traversalPending: boolean;
  mode: "browser" | "tauri";
  bootstrap: "loading" | "ready" | "unavailable";
  availability: Readonly<Record<BrowserCommandId, boolean>>;
}

export interface BrowserToolbarAdapter {
  getSnapshot(): BrowserToolbarSnapshot;
  subscribe(listener: (snapshot: BrowserToolbarSnapshot) => void): () => void;
  execute(envelope: BrowserCommandEnvelope): Promise<boolean>;
  start(): () => void;
}

export interface ShortcutEntry {
  commandId: BrowserCommandId;
  bindings: string[];
}

export interface NativeOwnedBinding {
  commandId: BrowserCommandId;
  binding: string;
}

export interface BrowserBootstrap {
  shortcutEntries: ShortcutEntry[];
  nativeOwnedBindings: NativeOwnedBinding[];
  hostCapabilities: {
    reloadDocumentation: boolean;
    openInDefaultBrowser: boolean;
  };
  runtimeGeneration: number;
}
