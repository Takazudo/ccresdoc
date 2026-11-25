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

Use the [retained app-only build procedure](verification-matrix.md#build-once-and-verify-the-retained-bundle-on-mac).
Run the package, Settings and launch controls smokes sequentially on that
exact bundle. Keep its absolute path, source SHA, executable hashes,
runtime-workspace token and digest, and dated macOS arm64 results.
Use synthetic settings and sources for any follow-up on the same bundle.

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

The bounded launch smoke uses
`bash scripts/test-launch.sh --bundle "$app_path" --controls`. Its observed
interactions cover ThemeToggle, native menu/Settings shortcuts, hide and
reactivate, Search result navigation, and Find controls. The Settings smoke
covers packaged configuration and runtime recovery. The broader checklist
above requires its own native results before changing the marker.
