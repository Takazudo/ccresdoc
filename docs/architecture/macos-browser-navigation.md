# macOS arm64 browser-navigation acceptance

This is a packaged-app host gate. Linux Chromium, Rust unit tests, and the
staged native probe are useful evidence, but none of them proves AppKit native
accelerator delivery or Tauri WebView/native arbitration.

## Verification marker

`macos-arm64-browser-chrome-native: PENDING (not run in the Linux worker)`

The release machine must replace `PENDING` with a dated host result (for
example `PASS:2026-09-01/<host>`) after completing every item below. Keep the
pending marker when no fresh macOS arm64 packaged result exists; do not infer a
packaged result from the Linux browser harness.

## Fresh-bundle procedure

Build a fresh bundle and run the existing package/runtime checks first:

```sh
bash scripts/build-macos-release.sh
APP_PATH=/absolute/path/to/the/fresh/CCResDoc.app
bash scripts/test-macos-package.sh --existing-bundle "$APP_PATH"
```

Run the following against that exact fresh `.app`, with an isolated temporary
settings/source fixture where a check needs generated content. Record the app
path, macOS version, arm64 host, date, and result beside the marker.

## Required native checks

1. **Primary menu accelerators:** invoke every default primary browser command
   once from the native menu, then change its binding in Settings and invoke
   the changed primary once. Confirm the menu labels/accelerators update and
   each action is delivered exactly once (no duplicate page/native dispatch).

2. **Alternate page binding:** configure an alternate binding for one command,
   press it in the docs WebView, and confirm the page-owned action invokes once.
   Confirm that a binding currently owned by the native menu is not also handled
   by the page.

3. **Settings capture arbitration:** begin shortcut capture and verify ambient
   dynamic accelerators are suspended. Cancel, save, and close the Settings
   window in turn; after each path confirm the previous/current native
   accelerators are restored exactly once and the docs window remains alive.

4. **App behavior and standard menus:** verify Back/Forward boundary and branch
   state, toolbar History/Home/Find/Search/Copy/More actions, controlled Find
   navigation and cleanup, validated external opening, Reload Documentation,
   relaunch persistence, and the standard application/Edit/Window menu actions
   (including Hide/Hide Others/Show All/Services and the Window submenu).

5. **Ordinary browser lifetime:** open the live docs URL with Open in Default
   Browser, confirm it works only while CCResDoc's loopback server is running,
   then quit CCResDoc and confirm the tab cannot be treated as a permanent
   hosted service. Confirm the ordinary page has no Tauri bridge and that
   Settings, Reload Documentation, and Open in Default Browser are unavailable.

The existing `scripts/test-launch.sh --controls` and
`pnpm run test:macos-settings` helpers can supply bounded packaged smoke and
Settings evidence, but they do not replace this checklist's native
once-only-arbitration and fresh-bundle result. Attach the native result to the
PR/release record; the Linux worker must report the marker as pending.
