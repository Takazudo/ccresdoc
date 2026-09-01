export const HISTORY_STATE_KEY = "__ccresdocBrowserHistoryV1";
const STORAGE_PREFIX = "ccresdoc:browser-history:v1:";

export interface ManagedHistoryState {
  scope: string;
  index: number;
}

export interface HistorySnapshot {
  boundary: number;
  current: number;
  maximum: number;
  canGoBack: boolean;
  canGoForward: boolean;
  traversalPending: boolean;
}

interface PersistedHistory {
  boundary: number;
  current: number;
  maximum: number;
  path: string;
}

export interface HistoryEnvironment {
  history: History;
  location: Location;
  sessionStorage: Storage;
}

function validPersisted(value: unknown): value is PersistedHistory {
  if (!value || typeof value !== "object") return false;
  const item = value as Partial<PersistedHistory>;
  return Number.isInteger(item.boundary)
    && Number.isInteger(item.current)
    && Number.isInteger(item.maximum)
    && typeof item.path === "string"
    && item.boundary! <= item.current!
    && item.current! <= item.maximum!;
}

function routerIndex(state: unknown): number | undefined {
  if (!state || typeof state !== "object") return undefined;
  const value = (state as { index?: unknown }).index;
  return Number.isInteger(value) ? value as number : undefined;
}

function managedState(state: unknown, scope: string): ManagedHistoryState | undefined {
  if (!state || typeof state !== "object") return undefined;
  const value = (state as Record<string, unknown>)[HISTORY_STATE_KEY];
  if (!value || typeof value !== "object") return undefined;
  const item = value as Partial<ManagedHistoryState>;
  return item.scope === scope && Number.isInteger(item.index)
    ? item as ManagedHistoryState
    : undefined;
}

export class ManagedHistoryController {
  readonly scope: string;
  private state: PersistedHistory;
  private pendingDirection: -1 | 1 | null = null;

  constructor(scope: string, private readonly env: HistoryEnvironment) {
    this.scope = scope;
    const state = env.history.state;
    const tagged = managedState(state, scope);
    const stored = this.readStored();
    const index = tagged?.index
      ?? routerIndex(state)
      ?? (stored?.path === this.stablePath() ? stored.current : 0);

    if (stored && tagged && tagged.index >= stored.boundary && tagged.index <= stored.maximum) {
      this.state = { ...stored, current: tagged.index, path: this.stablePath() };
    } else if (stored && !tagged && stored.path === this.stablePath()) {
      this.state = { ...stored, current: index };
    } else {
      this.state = { boundary: index, current: index, maximum: index, path: this.stablePath() };
    }
    this.tagCurrent(this.state.current);
    this.persist();
  }

  snapshot(): HistorySnapshot {
    const { boundary, current, maximum } = this.state;
    return {
      boundary,
      current,
      maximum,
      canGoBack: !this.pendingDirection && current > boundary,
      canGoForward: !this.pendingDirection && current < maximum,
      traversalPending: this.pendingDirection !== null,
    };
  }

  traverse(direction: -1 | 1): boolean {
    const snapshot = this.snapshot();
    if (direction < 0 ? !snapshot.canGoBack : !snapshot.canGoForward) return false;
    this.pendingDirection = direction;
    this.env.history.go(direction);
    return true;
  }

  /** Record a popstate immediately; the page-load event later settles the guard. */
  onPopState(state: unknown = this.env.history.state): boolean {
    const tagged = managedState(state, this.scope);
    const candidate = tagged?.index ?? routerIndex(state);
    if (candidate === undefined || candidate < this.state.boundary || candidate > this.state.maximum) {
      const recovery = candidate !== undefined && candidate < this.state.boundary ? 1 : -1;
      this.env.history.go(recovery);
      return false;
    }
    this.state.current = candidate;
    this.state.path = this.stablePath();
    this.tagCurrent(candidate);
    this.persist();
    return true;
  }

  /** Record the successful zfb transition, distinguishing traversal from a new branch. */
  settleSuccessfulNavigation(navigationType?: "push" | "replace" | "traverse"): void {
    const state = this.env.history.state;
    const tagged = managedState(state, this.scope);
    const indexed = routerIndex(state);
    const path = this.stablePath();
    const traversal = navigationType === "traverse" || this.pendingDirection !== null || tagged?.index !== undefined;

    if (traversal && tagged) {
      this.state.current = Math.min(this.state.maximum, Math.max(this.state.boundary, tagged.index));
    } else if (navigationType === "replace") {
      this.state.current = indexed ?? this.state.current;
    } else if (path !== this.state.path) {
      const next = indexed ?? this.state.current + 1;
      this.state.current = Math.max(this.state.boundary, next);
      this.state.maximum = this.state.current;
    }

    this.state.path = path;
    this.pendingDirection = null;
    this.tagCurrent(this.state.current);
    this.persist();
  }

  cancelTraversal(): void {
    this.pendingDirection = null;
  }

  private stablePath(): string {
    try {
      const path = decodeURI(this.env.location.pathname);
      return path === "/" ? "/docs/" : path;
    } catch {
      return this.env.location.pathname === "/" ? "/docs/" : this.env.location.pathname;
    }
  }

  private tagCurrent(index: number): void {
    const previous = this.env.history.state;
    const base = previous && typeof previous === "object" ? previous : {};
    this.env.history.replaceState({
      ...base,
      [HISTORY_STATE_KEY]: { scope: this.scope, index },
    }, "", this.env.location.href);
  }

  private readStored(): PersistedHistory | undefined {
    try {
      const raw = this.env.sessionStorage.getItem(`${STORAGE_PREFIX}${this.scope}`);
      if (!raw) return undefined;
      const value: unknown = JSON.parse(raw);
      return validPersisted(value) ? value : undefined;
    } catch {
      return undefined;
    }
  }

  private persist(): void {
    try {
      this.env.sessionStorage.setItem(`${STORAGE_PREFIX}${this.scope}`, JSON.stringify(this.state));
    } catch {
      // History remains correct for this page even when storage is unavailable.
    }
  }
}
