# Controlled Find and Search after zudo-doc 14

CCResDoc uses `@takazudo/zudo-doc@14.0.0` with one scoped consumer patch,
`app/patches/@takazudo__zudo-doc@14.0.0.patch`. Its SHA-256 is recorded in
`scripts/lib/toolchain-contract.mjs` and verified against the patch, lockfile,
workspace policy and installed package by `pnpm run check:dependencies`.
The independent compatibility fixture remains unpatched.

## Find in Page

zudo-doc 11 retired its Tauri-only Find export, and 14 has no
`@takazudo/zudo-doc/find-in-page` subpath. CCResDoc retains the controlled
native Find behavior in `app/src/browser-chrome/find-in-page/`. This is the
published 10.0.2 implementation with CCResDoc's former consumer patch applied.
Its source tag and local changes are recorded in `ZUDO_DEPS_PINS.md`.
The app controls the `Cmd/Ctrl+F` binding through its browser command adapter,
while the Find island opts out of its built-in shortcut. It excludes hidden,
chrome and control text, removes marks on route change, and restores the
original document text on close.

## Search patch

The 14.0.0 patch changes only the package's published SearchWidget,
search-widget-script and ZDTP loader files. SearchWidget gets a
`disableBuiltInShortcut` prop and WebKit-safe input attributes. The script
exposes open, close and refresh commands, suppresses its built-in `Cmd/Ctrl+K`
listener when CCResDoc owns shortcuts, rejects older in-flight index responses,
and cleans up listeners and timers when disconnected. It retains the package's
fetch, scoring, result rendering and paging engine. This preserves user-rebound
shortcuts and live search after native source publication.

The package's `zdtp-loader.js` is replaced with the disabled loader text the
package's own plugin would serve. CCResDoc deliberately runs with `plugins: []`
and has no `@takazudo/zdtp` peer because the design-token panel is off. Without
this shield, the static chrome import reaches an absent optional peer and the
node-free build fails.

The compatibility fixture tests unpatched upstream configuration. CCResDoc's
app tests exercise both the app-owned Find engine and the patched Search commands,
including shortcut ownership and stale response rejection. The runtime source
allowlist includes the patch and the app-owned Find files so frozen installs and
packaged bundles carry the same behavior.

Remove the Search patch only when an upstream release provides controlled
open/close/refresh and host shortcut opt-out, or after CCResDoc adopts an
app-owned Search widget with equivalent behavior. Remove the ZDTP shield only
when upstream makes the disabled import self-contained or CCResDoc enables and
bundles that peer.
