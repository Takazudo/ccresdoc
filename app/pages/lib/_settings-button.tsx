"use client";

import { computed, getScope, signal } from "@takazudo/zfb/zudo-react";

export type TauriGlobal = typeof globalThis & {
  __TAURI__?: { core?: { invoke?: (command: string) => Promise<unknown> } };
};

export function openSettingsFromDocs(
  root: TauriGlobal = globalThis as TauriGlobal,
): Promise<unknown> {
  const invoke = root.__TAURI__?.core?.invoke;
  if (typeof invoke !== "function") {
    return Promise.reject(new Error("Settings are available only in the CCResDoc app."));
  }
  return invoke("open_settings_window");
}

type Availability = "pending" | "available" | "unavailable";

export function SettingsHeaderButton() {
  // Server output and the pre-activation DOM are pending; availability is
  // decided only when this island activates in the browser.
  const availability = signal<Availability>("pending");
  getScope().onActivate(() => {
    availability.value = typeof (globalThis as TauriGlobal).__TAURI__?.core?.invoke === "function"
      ? "available"
      : "unavailable";
  });
  const pending = computed(() => availability.value === "pending");
  const unavailable = computed(() => availability.value === "unavailable");
  return (
    <button
      type="button"
      title="Settings"
      aria-label="Open Settings"
      aria-disabled={computed(() => (availability.value === "available" ? undefined : "true"))}
      data-zd-pending={computed(() => (pending.value ? "" : undefined))}
      disabled={unavailable}
      class="flex min-h-[2.75rem] min-w-[2.75rem] items-center justify-center rounded-[0.375rem] text-muted focus-visible:outline-2 focus-visible:outline-offset-2"
      on:click={() => {
        if (availability.value !== "available") return;
        void openSettingsFromDocs().catch((error) => {
          console.error("open_settings_window failed:", error);
        });
      }}
    >
      <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
        <path d="M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Z" />
        <path d="M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.86 2.86-.06-.06A1.7 1.7 0 0 0 15 19.4a1.7 1.7 0 0 0-1 .6 1.7 1.7 0 0 0-.4 1.1V21H9.6v-.1A1.7 1.7 0 0 0 8.5 19.4a1.7 1.7 0 0 0-1.88.34l-.06.06-2.86-2.86.06-.06A1.7 1.7 0 0 0 4.1 15a1.7 1.7 0 0 0-.6-1 1.7 1.7 0 0 0-1.1-.4H2V9.6h.4A1.7 1.7 0 0 0 4.1 8.5a1.7 1.7 0 0 0-.34-1.88l-.06-.06L6.56 3.7l.06.06A1.7 1.7 0 0 0 8.5 4.1a1.7 1.7 0 0 0 1-.6 1.7 1.7 0 0 0 .4-1.1V2h4v.4A1.7 1.7 0 0 0 15 4.1a1.7 1.7 0 0 0 1.88-.34l.06-.06 2.86 2.86-.06-.06A1.7 1.7 0 0 0 19.4 8.5a1.7 1.7 0 0 0 .6 1 1.7 1.7 0 0 0 1.1.4h.4v4h-.4A1.7 1.7 0 0 0 19.4 15Z" />
      </svg>
    </button>
  );
}

SettingsHeaderButton.displayName = "SettingsHeaderButton";
