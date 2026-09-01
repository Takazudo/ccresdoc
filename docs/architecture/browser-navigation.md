# Browser navigation chrome

CCResDoc has one command contract for the native window and an ordinary
browser tab. `app/src/browser-chrome/command-catalog.json` is the source of
truth for labels, menu placement, default bindings, and command identity. The
browser adapter receives the host bootstrap (configured bindings, native-owned
bindings, capabilities, and runtime generation), then dispatches toolbar,
overflow, configured-key, and native-menu envelopes through the same command
path. `Mod` means Control on Linux/Windows and Command on macOS.

The default page bindings are Back `Mod+[`, Forward `Mod+]`, Reload
Documentation `Mod+R`, Find `Mod+F`, and Search `Mod+K`. A changed or removed
Find/Search binding is applied to the page listener as well as the native menu;
the old package defaults must not remain active. An input, textarea, select,
contenteditable region, site-search input, Find input, or shortcut-capture
target owns its keystrokes and suppresses ambient page shortcuts.

## Ordinary browser mode

CCResDoc owns a loopback documentation server. The **Open in Default Browser**
action can open the current live `/docs/...` URL in the external browser only
while the app and its server remain running. Closing or quitting CCResDoc stops
that server, so an already-open ordinary browser tab cannot be treated as a
permanent hosted site. Ordinary browser pages do not receive a Tauri bridge or
privileged capabilities: History, Home, Find, Search, and Copy remain page
features, while Settings, Reload Documentation, and Open in Default Browser are
disabled. External navigation is subject to the ordinary browser's security
model; no Tauri command is available in this mode.

## Reload Documentation

Reload Documentation is a host operation, not an ordinary `location.reload()`.
The Tauri command authorizes the active main window, shows the loading document,
tears down the current runtime as part of the generation-guarded launch path,
starts the configured Rust generator/watchers and native zfb carrier, waits for
semantic `/docs/` readiness, and returns the main window to `/docs/`. A failed
attempt stays on the loading/error recovery surface. Browser mode leaves this
host-only command unavailable; use the browser's normal reload for a live tab
while the app/server is still running.

## Repeatable Chromium confirmation

The named root command starts the installed native zfb carrier directly on a
fresh loopback port, retries an address collision, waits for the rendered
`/docs/` shell and its module responses, and shuts down the exact process group
on success, failure, or interruption. It uses only the checked-in root,
Claude, Codex, and deterministic missing routes; it never copies a selected
local resource directory into the app. Browser interactions use real
Playwright `page.keyboard.press(...)` input, including default and custom
bindings. The Tauri bootstrap used in the page assertions is a deterministic
host seam; it does not claim to deliver a macOS native accelerator.

```sh
pnpm run test:browser-navigation-contract
pnpm --dir app install --frozen-lockfile
pnpm --dir app exec playwright install chromium
pnpm run test:browser-navigation
```

CI installs the pinned Chromium package with `--with-deps` and runs this
command in its own ten-minute step. `pnpm b4push` includes the same browser
step when Chromium is installed locally. The harness preflight also checks the
runtime source allowlist boundary, narrow Tauri capabilities, command catalog,
Playwright/lockfile pin, and the exact controlled Find/Search patch digest.

This Linux/Chromium confirmation proves rendered page dispatch and geometry,
managed history boundaries/branches, real patched Find and Search behavior,
More/menu focus, responsive targets, and ordinary-browser degradation. It does
not prove native macOS menu accelerator delivery, WebView behavior, or visual
parity of a packaged `.app`; those remain the macOS arm64 acceptance checklist.

## Find/Search patch boundary

The controlled package patch is retained only while the published zudo-doc
package lacks controlled Find open/close, controlled Search open/refresh,
host opt-outs for both built-in shortcuts, equivalent visible-content walker
filtering and cleanup, and the Find icon-control/complete-chrome offset hooks.
The exact hash and removal procedure are recorded in
[`zudo-doc-find-search-patch.md`](zudo-doc-find-search-patch.md). Do not remove
the patch by changing only the browser harness or by switching to package
internals.

The macOS host acceptance items and their honest run/pending marker are listed
in [`macos-browser-navigation.md`](macos-browser-navigation.md).
