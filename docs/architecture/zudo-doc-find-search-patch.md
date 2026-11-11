# Controlled Find and Search package patch

CCResDoc pins `@takazudo/zudo-doc` to `10.0.2` and applies one consumer-local
patch, `app/patches/@takazudo__zudo-doc@10.0.2.patch`. The patch SHA-256 and
the pnpm lock hash are both
`d6f52919667c2478934b2e35714e18220341dfd939c9b4f8f9a6317e0de2e5c9`. That value
is written by hand in `scripts/lib/toolchain-contract.mjs` (`ZUDO_DOC_PATCH`);
`pnpm run check:dependencies` compares the patch bytes, the workspace
registration, the lockfile entry, the patched importer resolution and the
installed patched files against it. No other package is patched: the earlier
`@takazudo/zfb-runtime` patch is gone, because zfb-runtime 6.0.1 handles a
skipped view transition itself and logs a genuine DOM-update error as
`[zfb] <name> <message> <stack>` (`test:browser-navigation` asserts both).

The compatibility probe in `compatibility/node-free-latest/` stays an unpatched
upstream consumer; its contract rejects any `patchedDependencies` entry.

## Scope 1 — controlled Find and Search

These hunks change only files reachable through the package's existing public
`./find-in-page`, `./search-widget`, and `./search-widget-script` exports:

- `dist/find-in-page/index.js` adds named open/close command functions, the
  command listener, the built-in `Cmd/Ctrl+F` opt-out, and idempotent route,
  swap, and teardown cleanup. `index.d.ts` declares that public contract and
  re-exports the existing engine from the public subpath for contract tests.
- `dist/find-in-page/find-in-page.js` rejects hidden, chrome, script/template,
  form-control, editable, closed/inert, and widget subtrees; checks connectivity before DOM
  mutation/traversal; and retains non-animated centered traversal.
- `dist/find-in-page/find-bar.js` (and its `.d.ts`) supplies the controlled bar marker, native
  search/helper attributes, live count, accessible icon controls, and narrow
  sizing hooks. CCResDoc owns the corresponding theme/offset CSS.
- `dist/search-widget/index.js` adds the built-in `Cmd/Ctrl+K` opt-out and the
  WebKit search/helper attributes. `index.d.ts` declares the prop.
- `dist/search-widget-script/index.js` exports named open, close, and refresh
  command functions and the stable command event. `index.d.ts` declares them.
- `dist/search-widget-script/generated-script.js` consumes the public command,
  refreshes only through element-owned methods, honors the shortcut opt-out,
  refreshes normal trigger-button opens, rejects superseded index responses,
  makes repeated dialog commands safe,
  and removes listeners/timers and restores document overflow on disconnect.
  It retains the shipped fetch, scoring, rendering, and paging engine.

## Scope 2 — `dist/zdtp-loader.js` stub for the node-free build

`zudo-doc` 10.0.2 ships `dist/zdtp-loader.js` as a re-export of the optional
peer `@takazudo/zdtp` plus a `configurePanelIfActive` helper. CCResDoc does not
install that peer, because the design-token panel is off.

Upstream shields the module with a plugin (`dist/plugins/zdtp-loader.js`,
registered by `dist/preset.js` when the panel is off) that serves the specifier
as a virtual module whose body is a single `throw`. CCResDoc's
`app/zfb.config.ts` ends `plugins: []` so that the build and `zfb dev` run no
Node plugin host, which drops that shield. The unresolvable specifier is then
reachable from the chrome through the design-token-panel bootstrap, and the
build fails.

The patch therefore replaces `dist/zdtp-loader.js` with upstream's own
`DISABLED_LOADER_SOURCE` string, byte for byte:

```js
throw new Error("@takazudo/zdtp is not bundled: designTokenPanel is false and bundleZdtp is not set, so this build shadows @takazudo/zudo-doc/zdtp-loader. If you reached this through @takazudo/zudo-doc/design-token-panel-bootstrap to mount your own panel, set bundleZdtp: true and install @takazudo/zdtp.");
export {};
```

This reproduces exactly what the upstream plugin would have served, keeps
`plugins: []` intact, and installs no package for a disabled feature. A
config-level alias would be the cleaner fix, but zfb 6.0.1 still exposes
aliasing only through the plugin `addAlias` hook, so no node-free config-level
remedy exists.

Remove this hunk once `dist/zdtp-loader.js` is self-shielding upstream, or once CCResDoc adopts the design
token panel and installs `@takazudo/zdtp`.

The app does not import package internals, copy the engines, reach into custom
element cache fields, or synthesize keyboard events. `app/pages/lib/_chrome.tsx`
mounts one Find instance and binds the public Search widget with both built-in
shortcuts disabled. `app/src/browser-chrome/adapter.ts` sends toolbar,
configured-key, and native-menu envelopes through the common dispatcher to the
public controllers.

The patch is included in the staged runtime source allowlist because the staged
frozen lockfile references it. The installed, patched package bytes remain part
of the staged dependency tree and workspace digest.

## Removal condition (Scope 1)

Remove the Scope 1 hunks only when a published zudo-doc release provides all of:
controlled Find open and close, controlled Search open and refresh, host opt-outs
for both built-in shortcuts, equivalent visible-content walker filtering and
cleanup guarantees, and compatible Find icon-control and complete-chrome offset
hooks. At that point, bump the exact package pin, migrate only to its documented
public APIs, remove `patchedDependencies` and the patch/hash validation, and
regenerate the frozen lockfile and runtime stage.

The removal condition remains unmet for 10.0.2: its public `find-in-page`,
`search-widget` and `search-widget-script` entries expose no open/close/refresh
commands, and neither built-in shortcut has a host opt-out.

The published package therefore still lacks controlled Find open/close,
controlled Search open/refresh, and host opt-outs for both built-in shortcuts.

## The optional `diff` edge

zudo-doc's `dist/diff-loader.js` reaches the optional peer `diff` through a
guarded `await import("diff")` that falls back to `null`. CCResDoc does not
install `diff` (the history panel is off), and the native zfb 6.0.1 build and
`zfb dev` both complete without it; the browser suites build the site through
`zfb build` and serve it with `zfb preview`. Install `diff` only if CCResDoc
enables the document-history diff view.

## Implementation guidance used

The CSS implementation follows `zudo-css-wisdom` guidance from
`states-and-transitions/hover-focus-active-states.mdx`,
`accessibility/prefers-reduced-motion.mdx`, and
`accessibility/form-control-styling.mdx`: hover is pointer-qualified, focus is
explicit, transitions are opt-in for no-preference, and forced-color controls
retain visible native color roles. Tauri integration follows
`frontend/find-in-page.mdx`, `frontend/webkit-form-autocomplete.mdx`, and
`rust-backend/menu-events.mdx`: host commands call a DOM engine directly,
app-internal search inputs opt out of WebKit helpers, and native menu envelopes
converge on the same page dispatcher.
