/** @jsxRuntime automatic */
/** @jsxImportSource preact */

import { act } from "preact/test-utils";
import { render } from "preact";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SearchShortcutBoundary } from "../pages/lib/_settings-button";

const mountedRoots: HTMLDivElement[] = [];
const removeListeners: Array<() => void> = [];

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

async function mountBoundary() {
  const root = document.createElement("div");
  document.body.append(root);
  mountedRoots.push(root);
  act(() => render(<SearchShortcutBoundary />, root));
  await flush();
}

function listenAtBubblePhase() {
  const listener = vi.fn();
  document.addEventListener("keydown", listener);
  removeListeners.push(() => document.removeEventListener("keydown", listener));
  return listener;
}

function press(key: "f" | "k", modifier: "ctrlKey" | "metaKey") {
  const event = new KeyboardEvent("keydown", {
    key,
    [modifier]: true,
    bubbles: true,
    cancelable: true,
  });
  document.body.dispatchEvent(event);
  return event;
}

afterEach(() => {
  for (const removeListener of removeListeners.splice(0)) removeListener();
  for (const root of mountedRoots.splice(0)) {
    act(() => render(null, root));
    root.remove();
  }
});

describe("search/find shortcut boundary", () => {
  it.each(["ctrlKey", "metaKey"] as const)(
    "blocks Cmd/Ctrl+F while the search dialog is open (%s)",
    async (modifier) => {
      const dialog = document.createElement("dialog");
      dialog.setAttribute("data-search-dialog", "true");
      dialog.open = true;
      document.body.append(dialog);
      await mountBoundary();

      const downstream = listenAtBubblePhase();
      const event = press("f", modifier);

      expect(event.defaultPrevented).toBe(true);
      expect(downstream).not.toHaveBeenCalled();
    },
  );

  it.each(["ctrlKey", "metaKey"] as const)(
    "blocks Cmd/Ctrl+K while the find bar is open (%s)",
    async (modifier) => {
      const input = document.createElement("input");
      input.setAttribute("aria-label", "Find in page");
      document.body.append(input);
      await mountBoundary();

      const downstream = listenAtBubblePhase();
      const event = press("k", modifier);

      expect(event.defaultPrevented).toBe(true);
      expect(downstream).not.toHaveBeenCalled();
    },
  );

  it.each(["ctrlKey", "metaKey"] as const)(
    "lets Cmd/Ctrl+F through when the search dialog is closed (%s)",
    async (modifier) => {
      const dialog = document.createElement("dialog");
      dialog.setAttribute("data-search-dialog", "true");
      dialog.open = false;
      document.body.append(dialog);
      await mountBoundary();

      const downstream = listenAtBubblePhase();
      const event = press("f", modifier);

      expect(event.defaultPrevented).toBe(false);
      expect(downstream).toHaveBeenCalledOnce();
    },
  );

  it.each(["ctrlKey", "metaKey"] as const)(
    "lets Cmd/Ctrl+K through when the find bar is closed (%s)",
    async (modifier) => {
      await mountBoundary();

      const downstream = listenAtBubblePhase();
      const event = press("k", modifier);

      expect(event.defaultPrevented).toBe(false);
      expect(downstream).toHaveBeenCalledOnce();
    },
  );
});
