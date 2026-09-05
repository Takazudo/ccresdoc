# CCResDoc architecture and verification contract

This is the consolidated integration record for the current `@takazudo/zfb`
2.15.1 / `@takazudo/zudo-doc` 5.17.2 application. CCResDoc is a native viewer
for selected Claude and Codex resources. The generated `/docs/` document is the
sole product landing surface; `/` is its exact server-rendered alias. The
product keeps permanent Claude and Codex top-level header categories.

## Landing and readiness

- `GET /` and `GET /docs/` return the same generated document shell. The logo
  is a direct `href="/docs/"` link and the header navigation keeps the
  permanent Claude and Codex category entries.
- The Tauri loading page remains visible until `GET /docs/` is HTTP 200 and
  contains the current `CCResDoc` shell marker plus Claude and Codex overview
  markers for the same transition generation. A generic 200 or a stale
  checked-in shell cannot release the loading page.
- Initial launch, Retry, and the View → Refresh menu all use the same
  generation-guarded boot path: resolve a writable workspace, generate from
  the canonical settings-selected sources (Claude on at `~/.claude`, Codex off
  at `~/.codex` by default), start the enabled Rust watcher(s), spawn the native
  zfb binary, wait for semantic readiness, then navigate to `/docs/`.
  A failed attempt emits the loading-page error state; a subsequent attempt
  tears down only its tracked sidecar and selects the preferred or fallback
  loopback port without touching a foreign listener.
- `$HOME` is never passed as the generator project root. The generator is
  scoped to the validated configured source. An unset or empty home is an
  explicit launch error when no absolute source override is available.
  `ZFB_DEV_BOOT_LAZY` is removed from the sidecar environment.

The host contract is exercised by `src-tauri` unit tests and by the staged and
packaged probes. The packaged probe creates a unique, valid-frontmatter
synthetic HOME fixture, waits for that fixture title in the *first accepted*
`/docs/` response, requests its generated route, launches twice, and checks
that quit frees port 4892. The fixture is created only after the bundle privacy
audit has completed.

## Generated theme assets and catalog

`@takazudo/zudo-doc/catalog` is the source of truth. Its schema-version-2
entries keep package metadata under `pack.meta`; the postinstall/prebuild
sync validates every metadata file, copies every current non-default `pack.css`
and its referenced fonts to `app/public/theme-packs/`, and writes `index.json`.
The runtime stage embeds the catalog/file list, then hashes every staged app and
dependency byte plus the staging/digest implementation into the generated
refresh token. Thus a route, catalog, CSS, font, package, config, or staging
change cannot reuse an older writable workspace. The verifier recomputes the
tree digest, and a focused test proves a `pages/` edit changes the token.

The frontend derives route settings, switcher order, and the typed registry
from the same catalog. The deterministic tests and staged probe verify that
the served index, each current stylesheet, every referenced font, and every
declared loaded font family agree. The default pack is metadata-only.

The theme head bootstrap restores the saved pack before paint, keeps a loading
attribute until its stylesheet settles, preserves the independent light/dark
state, and repairs the active pack head after soft navigation. Applying a pack
is atomic: a stylesheet error leaves the previous link, storage value, and
event untouched. Pack storage/events (`zudo-doc-theme-pack` /
`theme-pack-changed`) are intentionally independent from light/dark storage
(`zudo-doc-theme`). The hydrated `ThemePackSwitcher` is present in the docs
shell and browse-all loads the current catalog lazily.

## Ownership and intentional deviations

- zudo-doc owns sidebar/tree rendering, mobile and desktop toggles, theme
  controls, smart path breaks, and connector geometry. Its published resizer
  owns a 16px hitbox, 192–448px clamp, separator ARIA state, pointer/keyboard
  feedback, and width persistence. CCResDoc does not fork package CSS or
  navigation behavior.
- The Rust `ccresdoc-claude-md` generators/watchers own selected Claude- and
  Codex-resource generation in-process. This is intentional: it avoids a Node
  plugin host and keeps the packaged runtime node-free.
- Tauri owns find-in-page through the WebView/native host contract. It is not
  replaced by a Node plugin. The zfb config ends with `plugins: []`; the host
  owns the three route adapters because package route plugins require virtual
  modules and start Node.
- Node and pnpm are setup/build tools only. The staged runtime resolves the
  package-root carrier `node_modules/@takazudo/zfb-<platform>/zfb` directly,
  never `node_modules/.bin/zfb`, and excludes development tools, non-host
  binaries, and disabled plugin-host dependencies.

## Settings and isolation

The settings store resolves `CCRESDOC_CONFIG`, then
`XDG_CONFIG_HOME/ccresdoc/config.toml`, then `HOME/.config/ccresdoc/config.toml`.
Missing files use schema-version-1 defaults without creating TOML. Authored
source/preferred-port values are reported separately from canonical effective
source/active-port values; an occupied preferred loopback port uses a free
fallback only when `fallback_to_free_port` is true. Strict mode, malformed TOML,
semantic diagnostics, unsupported future versions, SHA-256 revision conflicts,
appearance authority, and saved-not-active restart failures retain Settings
recovery and never overwrite or signal foreign state.

The bounded fixture gate is deterministic and isolated:

```sh
bash scripts/test-macos-settings.sh --fixtures-only
CCRESDOC_SETTINGS_APP=/path/to/CCResDoc.app pnpm run test:macos-settings
```

Package mode passes a unique temporary `HOME`, `TMPDIR`, `XDG_CONFIG_HOME`, and
`CCRESDOC_CONFIG` through LaunchServices, creates a temporary source tree and
one exact foreign listener, and verifies docs readiness across relaunches,
fallback/effective ports, strict and bad-source recovery, malformed-byte
preservation, and app-owned child cleanup while the foreign listener remains
alive. It uses nonpersistent WebViews and refuses to run beside a preexisting
CCResDoc process. Production capabilities, generated permissions, bundle identity, native
driver hooks, and
fixture paths are statically guarded against test-only leakage. The manager's
packaged Computer Use pass verifies Settings focus/close/reopen,
picker/save/relaunch, appearance preview/cancel/persistence, external-edit
Reload/Reapply, caller/navigation denial, and visual no-flash behavior.

## Resource selection, generation, and package privacy

Schema version 1 has a `[resources]` table (`claude`, `codex`) and a `[source]`
table (`claude_dir`, `codex_dir`). The compatibility defaults are Claude on and
Codex off, but Claude-only, Codex-only, both-on, and both-off are all valid.
The coordinator always keeps the permanent top-level headers at positions 899
(Claude) and 904 (Codex). Claude details occupy 900–903 (`CLAUDE.md`, commands,
skills, agents); Codex details occupy 905–910 (`AGENTS.md`, config, agents,
hooks, rules, skills). Disabled sources publish a stable disabled overview and
prune their detail namespaces.

The Codex parser accepts root/project `AGENTS.md`, `config.toml`, agent TOML,
`hooks.json`, rules, and skill packages. It refuses broad roots, does not follow
arbitrary links, and permits only a direct symlink entry under `skills/` to
escape its configured directory. Managed output directories are real, scoped
directories under the docs root. Claude and Codex use disjoint detail
namespaces; the coordinator owns overview/status pages, candidate promotion,
rollback, stale-output pruning, and the readiness handshake. Each enabled
source owns one debounced watcher, and a transition is visible only when the
neutral shell and both overview markers agree on the current generation.

The runtime stage is a privacy boundary, not a copy of the developer checkout.
`scripts/stage-runtime-workspace.mjs` admits an explicit file-level source
allowlist, the generated theme catalog/assets, and no build `dist` tree. It
rejects every `claude-*`/`codex-*` detail/status namespace and `.ccresdoc-*`
transition namespace before copying; staged text is audited for synthetic
fixture sentinels and checkout/configured-root paths. The manifest records the
admitted inventory, excluded Node-only packages, direct native carrier, tree
digest/token, and privacy audit. The verifier, Linux native probe (including
rendered shell/404/HMR responses), and final macOS package script assert the
same contract over staged/final artifacts.

## Automated gates

The shared browser-chrome command catalog, page adapter, controlled Find/Search
surfaces, and host capability boundary are confirmed by the actual-key
Chromium harness described in
[`browser-navigation.md`](browser-navigation.md). The harness starts a real
native zfb dev server on an ephemeral loopback port, drives rendered routes with
Playwright `page.keyboard.press(...)`, and owns process-group teardown. Its
Tauri bootstrap is a deterministic page seam, so this evidence is intentionally
separate from native macOS menu delivery.

Run the focused current-toolchain gates with:

```sh
pnpm --dir app install --frozen-lockfile
pnpm --dir app run validate:dependencies
pnpm --dir app run validate:theme-packs
pnpm --dir app run typecheck
pnpm --dir app run check:zfb
pnpm --dir app run test:run
pnpm --dir app run test:theme-packs
pnpm --dir app exec playwright install chromium
pnpm run test:browser-navigation
pnpm --dir app exec zfb build
pnpm run test:runtime-digest
pnpm run test:runtime-files
pnpm run probe:runtime-package
pnpm --dir compatibility/node-free-latest install --frozen-lockfile
pnpm --dir compatibility/node-free-latest run evidence:check
pnpm --dir compatibility/node-free-latest run probe:matrix
bash scripts/check-node-free-compatibility.sh --check
pnpm b4push
```

The frontend suite covers route aliases/chrome, generated fixture semantics,
catalog/index/CSS/font parity, theme no-flash/bootstrap/switcher hydration,
failed-pack rollback, font-bearing replacement, independent storage/events,
and package-owned sidebar resizer DOM behavior. Rust tests cover formatting,
generation/watch/live smoke, no-home scoping, readiness classification,
boot-lazy neutralization, refresh/retry generations, workspace refresh tokens,
native binary resolution, and process-group teardown.

`pnpm b4push` is the repository's real ten-step sequence: zfb pin check; frozen
app install and installed-tree validation; app typecheck/zfb check/Vitest; Rust
fmt; Linux-boundary clippy and tests with `--exclude ccresdoc`; native zfb build;
the staged runtime digest/verification and serialized two-launch probe; the
frozen compatibility evidence/package/config/check/build fixture; and the
actual-key Chromium browser-navigation confirmation. Install Chromium before
the local b4push run; CI provisions the pinned browser with `--with-deps` in a
separate step. Theme-pack validation and its standalone Node test are explicit
focused gates above and are not silently implied by b4push's Vitest step.

The staged runtime gate is:

```sh
pnpm run probe:runtime-package
```

It stages a lockfile-faithful workspace, checks the dynamic theme catalog and
served assets, asserts the exact root/docs shell and semantic `/docs/` readiness,
exercises content HMR, samples the process group with a recording/failing Node
sentinel, rejects `plugin-host.mjs`, and proves two clean launches with port 4892
released between attempts. Its static checks also recompute the SHA-256 tree
`sha256-tree-v1` digest and refresh token, verify the direct package-root native
carrier against canonical package facts, and reject forbidden runtime packages.

The packaged macOS arm64 gate is a separate mandatory host-only check. It uses an
explicit fresh target directory and exact bundle shape, then launches the real
app/WebView twice with generated fixture content, HMR, a failing Node sentinel,
and port cleanup. The script prints both paths on success:

```sh
scripts/test-macos-package.sh
# $CARGO_TARGET_DIR/release/bundle/macos/CCResDoc.app
```

It must be run on macOS arm64 and must not be pointed at an installed app. On
Linux the script reports the host-gate skip; Linux can run the staged native
probe and static package assertions but cannot claim the Tauri/WebView launch.
The additional native browser-chrome acceptance items (once-only primary and
alternate accelerator arbitration, Settings capture suspension/restoration,
full menu/action coverage, and ordinary-browser lifetime) are recorded in the
[`macOS browser-navigation checklist`](macos-browser-navigation.md). Its
`macos-arm64-browser-chrome-native` marker remains `PENDING` until a fresh
macOS arm64 release-machine result replaces it.

## Residual visual handoff

The final Computer Use pass remains a post-merge release check on the exact
fresh bundle: 1280×1049, native 1200×800, and a narrower desktop viewport;
light/dark and two materially different font-bearing packs; reload/relaunch
persistence; resizer pointer/keyboard/focus/feedback/clamp; soft navigation;
and clean quit. Unit tests and the package probe are deterministic contracts,
but they do not substitute for native WebView visual inspection.
