import { describe, expect, it, vi } from "vitest";
import { HISTORY_STATE_KEY, ManagedHistoryController, type HistoryEnvironment } from "../src/browser-chrome/history";

function environment(path = "/docs/a/") {
  let state: unknown = { index: 4 };
  const values = new Map<string, string>();
  const go = vi.fn();
  const env = {
    history: {
      get state() { return state; },
      replaceState(next: unknown) { state = next; },
      go,
    } as unknown as History,
    location: { pathname: path, href: `http://localhost${path}` } as Location,
    sessionStorage: {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => { values.set(key, value); },
    } as Storage,
  } satisfies HistoryEnvironment;
  return {
    env,
    go,
    setPath(next: string) { (env.location as { pathname: string; href: string }).pathname = next; (env.location as { href: string }).href = `http://localhost${next}`; },
    setState(next: unknown) { state = next; },
    getState: () => state as Record<string, unknown>,
  };
}

describe("managed browser history", () => {
  it("establishes and preserves a deep-route boundary across remounts", () => {
    const test = environment();
    const first = new ManagedHistoryController("tauri:7", test.env);
    expect(first.snapshot()).toMatchObject({ boundary: 4, current: 4, maximum: 4, canGoBack: false });
    expect(test.getState()[HISTORY_STATE_KEY]).toEqual({ scope: "tauri:7", index: 4 });

    const remounted = new ManagedHistoryController("tauri:7", test.env);
    expect(remounted.snapshot()).toMatchObject({ boundary: 4, current: 4, maximum: 4 });
  });

  it("truncates a forward branch after A to B to C, Back to B, then D", () => {
    const test = environment("/docs/a/");
    const history = new ManagedHistoryController("browser:tab", test.env);
    test.setPath("/docs/b/"); test.setState({ index: 5 }); history.settleSuccessfulNavigation("push");
    test.setPath("/docs/c/"); test.setState({ index: 6 }); history.settleSuccessfulNavigation("push");
    expect(history.snapshot()).toMatchObject({ current: 6, maximum: 6, canGoBack: true, canGoForward: false });

    test.setPath("/docs/b/");
    test.setState({ index: 5, [HISTORY_STATE_KEY]: { scope: "browser:tab", index: 5 } });
    expect(history.onPopState()).toBe(true);
    history.settleSuccessfulNavigation("traverse");
    expect(history.snapshot().canGoForward).toBe(true);

    test.setPath("/docs/d/"); test.setState({ index: 6 }); history.settleSuccessfulNavigation("push");
    expect(history.snapshot()).toMatchObject({ current: 6, maximum: 6, canGoForward: false });
  });

  it("guards repeated traversal and rejects crossing the managed boundary", () => {
    const test = environment();
    const history = new ManagedHistoryController("tauri:9", test.env);
    test.setPath("/docs/b/"); test.setState({ index: 5 }); history.settleSuccessfulNavigation("push");

    expect(history.traverse(-1)).toBe(true);
    expect(history.traverse(-1)).toBe(false);
    expect(test.go).toHaveBeenCalledOnce();
    expect(history.snapshot().traversalPending).toBe(true);

    test.setState({ index: 3 });
    expect(history.onPopState()).toBe(false);
    expect(test.go).toHaveBeenLastCalledWith(1);
  });

  it("uses a new boundary for a new runtime generation", () => {
    const test = environment();
    const oldGeneration = new ManagedHistoryController("tauri:10", test.env);
    test.setPath("/docs/b/"); test.setState({ index: 5 }); oldGeneration.settleSuccessfulNavigation("push");
    const nextGeneration = new ManagedHistoryController("tauri:11", test.env);
    expect(nextGeneration.snapshot()).toMatchObject({ boundary: 5, current: 5, maximum: 5 });
  });
});
