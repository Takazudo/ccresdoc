# Controlled Find and Search package patch

CCResDoc pins `@takazudo/zudo-doc` to `5.12.1` and applies the consumer-local
patch at `app/patches/@takazudo__zudo-doc@5.12.1.patch`. The patch SHA-256 and
pnpm lock hash are both
`845bacae4edff6b516c1a26ac5d15d07ed4583f0dd908a883661be56463cbe53`.
`scripts/validate-dependencies.mjs` checks the version, registration, patch
bytes, lock resolution, and installed public controllers.

The patch changes only files reachable through the package's existing public
`./find-in-page`, `./search-widget`, and `./search-widget-script` exports:

- `dist/find-in-page/index.js` adds named open/close command functions, the
  command listener, the built-in `Cmd/Ctrl+F` opt-out, and idempotent route,
  swap, and teardown cleanup. `index.d.ts` declares that public contract and
  re-exports the existing engine from the public subpath for contract tests.
- `dist/find-in-page/find-in-page.js` rejects hidden, chrome, script/template,
  form-control, editable, closed/inert, and widget subtrees; checks connectivity before DOM
  mutation/traversal; and retains non-animated centered traversal.
- `dist/find-in-page/find-bar.js` supplies the controlled bar marker, native
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

The app does not import package internals, copy the engines, reach into custom
element cache fields, or synthesize keyboard events. `app/pages/lib/_chrome.ts`
mounts one Find instance and binds the public Search widget with both built-in
shortcuts disabled. `app/src/browser-chrome/adapter.ts` sends toolbar,
configured-key, and native-menu envelopes through the common dispatcher to the
public controllers.

The patch is included in the staged runtime source allowlist because the staged
frozen lockfile references it. The installed, patched package bytes remain part
of the staged dependency tree and workspace digest.

## Removal condition

Remove this patch only when a published zudo-doc release provides all of:
controlled Find open and close, controlled Search open and refresh, host opt-outs
for both built-in shortcuts, equivalent visible-content walker filtering and
cleanup guarantees, and compatible Find icon-control and complete-chrome offset
hooks. At that point, bump the exact package pin, migrate only to its documented
public APIs, remove `patchedDependencies` and the patch/hash validation, and
regenerate the frozen lockfile and runtime stage.

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
