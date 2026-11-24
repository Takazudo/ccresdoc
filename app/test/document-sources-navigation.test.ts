import { describe, expect, it, vi } from "vitest";
import { installUnsavedNavigationGuard } from "../src/document-sources/navigation-guard";

describe("source editor unsaved navigation", () => {
  it("keeps a draft when another document link is rejected", () => {
    document.body.innerHTML = `<a href="/docs/other/">Other</a>`;
    const confirmLeave = vi.fn(() => false);
    const stop = installUnsavedNavigationGuard({ win: window, doc: document, isDirty: () => true, confirmLeave });
    const event = new MouseEvent("click", { bubbles: true, cancelable: true });
    document.querySelector("a")!.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    expect(confirmLeave).toHaveBeenCalledOnce();
    stop();
  });

  it("reverses Back before the router can swap away from the draft", () => {
    const initial = window.history.state;
    window.history.replaceState({ index: 4 }, "", window.location.href);
    const go = vi.spyOn(window.history, "go").mockImplementation(() => {});
    const confirmLeave = vi.fn(() => false);
    const stop = installUnsavedNavigationGuard({ win: window, doc: document, isDirty: () => true, confirmLeave });
    const event = new PopStateEvent("popstate", { state: { index: 3 } });
    window.dispatchEvent(event);
    expect(go).toHaveBeenCalledWith(1);
    expect(confirmLeave).toHaveBeenCalledOnce();
    stop(); go.mockRestore(); window.history.replaceState(initial, "", window.location.href);
  });

  it("restores an unindexed Forward entry and still guards the next Back", () => {
    const initial = window.history.state;
    const initialUrl = window.location.href;
    window.history.replaceState({ index: 4 }, "", "/docs/ccresdoc/document-sources/");
    const editorUrl = window.location.href;
    const go = vi.spyOn(window.history, "go").mockImplementation(() => {});
    const push = vi.spyOn(window.history, "pushState");
    const confirmLeave = vi.fn(() => false);
    const stop = installUnsavedNavigationGuard({ win: window, doc: document, isDirty: () => true, confirmLeave });
    try {
      window.history.replaceState(null, "", "/docs/other/");
      window.dispatchEvent(new PopStateEvent("popstate", { state: null }));
      expect(go).not.toHaveBeenCalled();
      expect(push).toHaveBeenCalledWith({ index: 4 }, "", editorUrl);
      expect(window.location.href).toBe(editorUrl);
      window.dispatchEvent(new PopStateEvent("popstate", { state: { index: 3 } }));
      expect(confirmLeave).toHaveBeenCalledTimes(2);
      expect(go).toHaveBeenCalledWith(1);
    } finally {
      stop(); push.mockRestore(); go.mockRestore();
      window.history.replaceState(initial, "", initialUrl);
    }
  });

  it("rejects a native toolbar or menu command before traversal", () => {
    const stop = installUnsavedNavigationGuard({ win: window, doc: document, isDirty: () => true, confirmLeave: () => false });
    const event = new Event("ccresdoc:before-document-navigation", { cancelable: true });
    document.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    stop();
  });
});
