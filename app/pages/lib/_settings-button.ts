"use client";

import { h } from "preact";
import { useEffect } from "preact/hooks";

export type TauriGlobal = typeof globalThis & {
  __TAURI__?: { core?: { invoke?: (command: string) => Promise<unknown> } };
};

const SEARCH_DIALOG_SELECTOR = "dialog[data-search-dialog]";
const FIND_IN_PAGE_INPUT_SELECTOR = 'input[aria-label="Find in page"]';

export function SearchShortcutBoundary() {
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey)) return;

      if (event.key === "f") {
        const searchDialog = document.querySelector<HTMLDialogElement>(SEARCH_DIALOG_SELECTOR);
        if (!searchDialog?.open) return;
      } else if (event.key === "k") {
        if (!document.querySelector<HTMLInputElement>(FIND_IN_PAGE_INPUT_SELECTOR)) return;
      } else {
        return;
      }

      event.preventDefault();
      event.stopImmediatePropagation();
    };

    document.addEventListener("keydown", handleKeyDown, true);
    return () => document.removeEventListener("keydown", handleKeyDown, true);
  }, []);
  return null;
}

SearchShortcutBoundary.displayName = "SearchShortcutBoundary";

export function openSettingsFromDocs(
  root: TauriGlobal = globalThis as TauriGlobal,
): Promise<unknown> {
  const invoke = root.__TAURI__?.core?.invoke;
  if (typeof invoke !== "function") {
    return Promise.reject(new Error("Settings are available only in the CCResDoc app."));
  }
  return invoke("open_settings_window");
}

export function SettingsHeaderButton() {
  return h(
    "button",
    {
      type: "button",
      title: "Settings",
      "aria-label": "Open Settings",
      class: "flex min-h-11 min-w-11 items-center justify-center rounded-md text-muted focus-visible:outline-2 focus-visible:outline-offset-2",
      onClick: () => {
        void openSettingsFromDocs().catch((error) => {
          console.error("open_settings_window failed:", error);
        });
      },
    },
    h(
      "svg",
      { viewBox: "0 0 24 24", width: 20, height: 20, fill: "none", stroke: "currentColor", "stroke-width": 2, "aria-hidden": "true" },
      h("path", { d: "M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Z" }),
      h("path", { d: "M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.86 2.86-.06-.06A1.7 1.7 0 0 0 15 19.4a1.7 1.7 0 0 0-1 .6 1.7 1.7 0 0 0-.4 1.1V21H9.6v-.1A1.7 1.7 0 0 0 8.5 19.4a1.7 1.7 0 0 0-1.88.34l-.06.06-2.86-2.86.06-.06A1.7 1.7 0 0 0 4.1 15a1.7 1.7 0 0 0-.6-1 1.7 1.7 0 0 0-1.1-.4H2V9.6h.4A1.7 1.7 0 0 0 4.1 8.5a1.7 1.7 0 0 0-.34-1.88l-.06-.06L6.56 3.7l.06.06A1.7 1.7 0 0 0 8.5 4.1a1.7 1.7 0 0 0 1-.6 1.7 1.7 0 0 0 .4-1.1V2h4v.4A1.7 1.7 0 0 0 15 4.1a1.7 1.7 0 0 0 1.88-.34l.06-.06 2.86 2.86-.06-.06A1.7 1.7 0 0 0 19.4 8.5a1.7 1.7 0 0 0 .6 1 1.7 1.7 0 0 0 1.1.4h.4v4h-.4A1.7 1.7 0 0 0 19.4 15Z" }),
    ),
  );
}
