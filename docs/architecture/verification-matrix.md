# CCResDoc architecture and verification contract

CCResDoc uses the published zfb 7.2.0 family and `@takazudo/zudo-doc` 14.0.0.
The main WebView serves documentation for the active source catalog.

## Landing and readiness

- `GET /` and `GET /docs/` serve the same generated document. The logo
  links to `/docs/` and navigation reflects the current mounts.
- The host checks the docs shell and current generation markers for the
  app-owned manager and every active source mount. Same-origin island entry
  bundles referenced by readiness pages must also be available. A generic 200
  or a stale shell cannot release the loading screen.
- Initial launch, Retry and View → Refresh use generation-guarded workspace,
  source publication, watcher, and native zfb startup. Each attempt owns its
  sidecar. An occupied loopback port can select a free fallback.
- The host reads only validated configured source and instructions scan roots.
  It removes `ZFB_DEV_BOOT_LAZY` from the sidecar environment.

## Generated theme assets and catalog

`@takazudo/zudo-doc/catalog` is the source of truth. Its schema-version-2
entries keep package metadata under `pack.meta`; the postinstall/prebuild
sync validates every metadata file, copies every current stylesheet pack `pack.css`
and its referenced fonts to `app/public/theme-packs/`, and writes `index.json`.
The runtime stage embeds the catalog/file list, then hashes every staged app and
dependency byte plus the staging/digest implementation into the generated
refresh token. Thus a route, catalog, CSS, font, package, config, or staging
change cannot reuse an older writable workspace. The verifier recomputes the
tree digest, and a focused test proves a `pages/` edit changes the token.

The frontend derives route settings, switcher order, and the typed registry
from the same catalog. The deterministic tests and staged probe verify that
the served index, each current stylesheet, every referenced font, and every
declared loaded font family agree. The configured `classic` pack is metadata-only (`hasStylesheet: false`).

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
- The Rust `ccresdoc-claude-md` generators/watchers own mounted document-source generation in-process. This is intentional: it avoids a Node
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
Missing files use schema-v3 defaults without creating TOML. Valid v2 files
migrate atomically and idempotently. A
recognized schema-v1 file is retired once at startup (rechecked before
removal); malformed, unreadable, unrecognized and future files are protected. Authored
source/preferred-port values are reported separately from canonical effective
source/active-port values; an occupied preferred loopback port uses a free
fallback only when `fallback_to_free_port` is true. Strict mode, malformed TOML,
semantic diagnostics, unsupported future versions, SHA-256 revision conflicts,
appearance authority, and saved-not-active restart failures retain Settings
recovery and never overwrite or signal foreign state.

The bounded fixture gate is deterministic and isolated:

```sh
bash scripts/test-macos-settings.sh --fixtures-only
node --test scripts/macos-acceptance.test.mjs
CCRESDOC_SETTINGS_APP=/absolute/path/to/CCResDoc.app pnpm run test:macos-settings
```

The Settings smoke exercises schema-v2 migration with synthetic v2 inputs.
Package mode passes a unique temporary `HOME`, `TMPDIR`, `XDG_CONFIG_HOME`, and
`CCRESDOC_CONFIG` through LaunchServices, creates a temporary source tree and
one exact foreign listener, and verifies docs readiness across relaunches,
fallback/effective ports, strict and bad-source recovery, malformed-byte
preservation, and app-owned child cleanup while the foreign listener remains
alive. It uses nonpersistent WebViews and refuses to run beside a preexisting
CCResDoc process. Production capabilities, generated permissions, bundle identity, native
driver hooks, and fixture paths are statically guarded against test-only
leakage. The separate manual Settings walkthrough covers picker/save/relaunch,
appearance preview/cancel/persistence, external-edit Reload/Reapply,
caller/navigation denial, and visual first-paint behavior.

## Source catalog, generation, and package privacy

Schema v3 stores `[[document_sources]]` and reusable `[[user_presets]]`.
Each source has a stable ID, unique mount, configured root, optional separate
instructions scan root, and a versioned built-in or custom rule snapshot.
Presets carry reusable reading rules without user paths. The initial catalog
enables Claude Code at `~/.claude` and leaves Codex at `~/.codex` disabled.
The app-owned article at `/docs/ccresdoc/document-sources/` remains available
with zero enabled sources or after first-launch source generation failure.

Each enabled source owns a watcher. A private manifest scopes transactional
promotion, rollback, stale output pruning and search. Generated routes are
scoped to the source mount. The search index associates each entry with a
source ID and route. Authored paths and catalog state stay in the trusted
native/editor boundary. Built-in Codex rules recognize instructions, config,
agents, hooks, rules and skill packages, including the narrow direct skill
symlink case. Custom rule snapshots define Markdown, code or skill input.

`scripts/stage-runtime-workspace.mjs` copies explicitly allowed app files and
generated theme assets. The build `dist`, generated source namespaces,
and private transition state are excluded. Staged text and served output are
checked for synthetic fixture and authored-root leakage. The manifest carries
the admitted inventory, excluded packages, native carrier and digest/token.

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
native binary resolution, and process-group teardown. The host's Tauri-free
`settings.rs`, `appearance.rs` and `runtime.rs` (port choice, origin/navigation
policy, semantic readiness, supersession and serialized apply) run through
`crates/ccresdoc-host-domain` in the Linux workspace gate; that is runtime-policy
coverage, not a Tauri app build or native WebView/FSEvents/packaged acceptance.

`pnpm b4push` is the repository's real ten-step sequence: zfb pin check; frozen
app install and installed-tree validation; app typecheck/zfb check/Vitest and
the Settings Node tests; Rust
fmt; Linux-boundary clippy and tests with `--exclude ccresdoc`; native zfb build;
the staged runtime digest/verification and serialized two-launch probe; the
frozen compatibility evidence/package/config/check/build fixture; and the
actual-key Chromium browser-navigation confirmation. Install Chromium before
the local b4push run. CI strictly installs the pinned Chromium with
`playwright install chromium`, then runs a separate best-effort
`playwright install-deps chromium` step for Ubuntu system packages (a warning,
not a failure); the browser-navigation step must still launch Chromium and
pass. That apt step is Linux CI provisioning, not a macOS setup step. Theme-pack validation and its standalone Node test are explicit
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

## Build once and verify the retained bundle on Mac

Use macOS 27 or newer on arm64, a logged-in GUI session, the repository tool
pins, and a terminal with Accessibility and System Events UI scripting access.
Close any existing CCResDoc instance. Run from a clean checkout at the exact
candidate SHA. The app-only build stays in the caller-owned Cargo target
directory so every probe can use the same bundle.

```bash
set -euo pipefail
task_dir="$(mktemp -d "${TMPDIR:-/tmp}/ccresdoc-acceptance.XXXXXX")"
task_dir="$(cd "$task_dir" && pwd -P)"
export CARGO_TARGET_DIR="$task_dir/cargo-target"
evidence_dir="$task_dir/evidence"

{
  git rev-parse HEAD
  node --version
  pnpm --version
  rustc --version
  cargo tauri --version
} | tee "$task_dir/build-input.txt"
pnpm --dir app install --frozen-lockfile
cargo tauri build --bundles app 2>&1 | tee "$task_dir/build.log"
app_path="$CARGO_TARGET_DIR/release/bundle/macos/CCResDoc.app"
bash scripts/test-macos-acceptance.sh "$app_path" "$evidence_dir"
```

The runner records the source SHA, host and tool versions, absolute bundle
path, executables, full contents fingerprint, and runtime workspace token and
digest. It runs these commands in order and compares bundle identity after
each checker, including a failed checker.

```bash
bash scripts/test-macos-package.sh --existing-bundle "$app_path"
CCRESDOC_SETTINGS_APP="$app_path" pnpm run test:macos-settings
bash scripts/test-launch.sh --bundle "$app_path" --controls
```

The package smoke audits the staged bundle and launches the real app twice.
It verifies an authored synthetic route, the Node sentinel, and sidecar
shutdown. Settings exercises migration, recovery and port ownership. The
launch smoke checks the owned app, manager, a synthetic source at a variable
mount, and the live search index. Its Accessibility pass observes ThemeToggle,
the menus and Settings shortcut, Command-H, and Command-K/Command-F. It also
adds and removes a synthetic skill and checks both search-index transitions.
The probes inject isolated LaunchServices paths and a nonpersistent WebView
setting. A missing native capability yields SKIP or BLOCKED evidence.

The default `bash scripts/test-macos-package.sh` builds an ephemeral app, whose
printed path is removed on exit. Use the retained workflow for follow-up
checks. Keep the evidence directory, build inputs and build log with the result, including
failure details and any retained runtime directory.

Portable/unit checks, actual-key Chromium, and the staged native runtime
probe cover different parts of the contract. The bounded macOS smoke leaves
the broader [native browser-navigation checklist](macos-browser-navigation.md),
historical viewport walkthrough, and cold-compilation cancellation matrix
pending until their respective acceptance records are complete.

## Residual visual handoff

The final Computer Use pass remains a post-merge release check on the exact
fresh bundle: 1280×1049, native 1200×800, and a narrower desktop viewport;
light/dark and two materially different font-bearing packs; reload/relaunch
persistence; resizer pointer/keyboard/focus/feedback/clamp; soft navigation;
and clean quit. Unit tests and the package probe are deterministic contracts,
but they do not substitute for native WebView visual inspection.
