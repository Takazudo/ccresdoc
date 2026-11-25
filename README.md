# CCResDoc

A macOS documentation viewer for configured local document sources. The initial catalog contains an enabled Claude Code source and a disabled Codex source; additional sources can use built-in or user-defined reading presets. The native Tauri host runs a node-free `zfb` sidecar and Rust generators/watchers after packaging.

## Architecture

```
Configured source roots + optional instructions scan roots
     │
     ▼  Rust source catalog, generators, watchers and ownership manifest
app/src/content/docs/<mount>/  ← generated MDX for enabled sources
     │
     ▼  native zfb dev on the selected loopback port
WKWebView → /docs/ and /docs/ccresdoc/document-sources/
```

The app-owned source manager is a normal zudo-doc article. Settings remains a separate native window for appearance, shortcuts, server options, diagnostics and protected-config recovery. Its Resources section links to the article. Sources hold stable IDs, labels, enabled state, paths, unique mounts and independent rule snapshots. Presets contain reusable rules only. Changes are saved with revision checks, then applied to the runtime; generation and search publication use the same private ownership manifest. Disabled or removed sources retain their original files and lose only app-owned routes and search entries.

The app and compatibility fixture have independent frozen lockfiles. The `@takazudo/zfb*` family is pinned to `7.2.0`, `@takazudo/zudo-doc` to `14.0.0`, and the app carries a reviewed zudo-doc patch for native Find. The native runtime uses no Node host or package-owned routes. A staged, digest-checked writable workspace admits only release-owned routes and assets, not generated documents or source paths. The preferred port defaults to 4892; a free loopback fallback may be selected when occupied. The host waits for the app-owned manager and every enabled source to become semantically ready before showing the document WebView. First-launch content failures retain the manager route and Settings recovery.

## Open in an ordinary browser

The app owns a loopback documentation server. **Open in Default Browser** opens
the current live `/docs/...` URL only while CCResDoc and that server remain
running; quitting the app ends that live URL. An ordinary browser page has no
Tauri bridge or privileged capability. History, Home, Find, Search, and Copy
remain available there, while Settings, Reload Documentation, and Open in
Default Browser are disabled. Reload Documentation is a native host restart
and semantic-readiness flow, not a normal browser `location.reload()`.

The shared toolbar/shortcut contract, actual-key browser harness, Find patch
boundary, and macOS arm64 acceptance marker are documented in
[`docs/architecture/browser-navigation.md`](docs/architecture/browser-navigation.md)
and [`docs/architecture/macos-browser-navigation.md`](docs/architecture/macos-browser-navigation.md).

## Settings contract

Settings are a human-editable TOML document. The path is resolved in this order:

1. `CCRESDOC_CONFIG` when it is set to a non-empty path;
2. `$XDG_CONFIG_HOME/ccresdoc/config.toml` when `XDG_CONFIG_HOME` is non-empty;
3. `$HOME/.config/ccresdoc/config.toml`.

Reading a missing file never creates it. The current schema is version 3. A fresh catalog starts with Claude Code enabled at `~/.claude` and Codex disabled at `~/.codex`. Each `[[document_sources]]` entry stores its ID, label, root, optional separate instructions scan root, mount, enabled state, preset origin and versioned rule snapshot. `[[user_presets]]` entries store rule definitions without paths or source identity. `[appearance]`, `[server]` and `[shortcuts]` remain app settings. Schema-v2 files migrate atomically and idempotently, preserving authored paths, enabled states and other settings; recognized schema-v1 retirement remains separate. Built-in Claude/Codex rules keep their specialized renderers, while custom markdown, code and skill rules use public include/exclude mappings.

Use **Edit document sources…** in native Settings or the link on the docs home page to manage sources and presets. Save persists a draft; **Apply saved** generates and activates it. Invalid or missing roots, duplicate mounts and output collisions are validated before publication. A failed apply reports saved-but-not-active while the previous valid output remains available. If a source fails on first launch, the app-owned manager remains reachable for repair.

The editor reports authored values separately from the effective/active runtime.
The authored source and preferred port remain exactly what is in TOML; the
effective source is canonicalized and the effective port can differ when the
preferred loopback port is occupied. CCResDoc only binds its own loopback
listener and never signals or kills a foreign port owner. Set
`fallback_to_free_port = false` for strict mode; an occupied preferred port
then leaves the saved document untouched and exposes the bundled Settings
recovery surface.

Missing settings use defaults without writing TOML. Parseable semantic errors
(for example a relative or unavailable source, invalid mode, or out-of-range
port) are shown as blocking diagnostics and use safe effective defaults until
repaired. Malformed TOML is preserved byte-for-byte and requires the explicit
Replace malformed action. An unsupported future `schema_version` is read-only;
CCResDoc will not overwrite it. A valid but unavailable theme pack is retained
as authored data while the effective theme falls back to `classic`. Existing
comments, unknown keys, newline style, and file permissions are preserved on a
successful atomic save.

Settings opens from the docs gear, native menu, Command-comma and loading/error recovery. It contains app settings and a Resources summary with a link to the source-management article. Appearance changes can update the page without restarting the sidecar. Source, port and fallback changes require runtime apply. The source article maintains its own draft, supports conflict reload/rebase and preserves unsaved changes when navigation is attempted. Native Settings and the article save their respective scopes with a SHA-256 revision check, so a stale window cannot overwrite the other scope.

Malformed TOML and unsupported future schema versions remain protected and require explicit recovery. Unknown keys, comments, newline style and file permissions survive valid atomic saves. Schema-v2 migration preserves its Claude/Codex paths and selections; the older recognized-v1 retirement policy is unchanged. Source directories are read only. The main document WebView receives only narrow, active-origin source-editor commands on the app-owned article. Other pages and stale runtime generations do not receive Settings or source-management authority.

### Settings verification

The deterministic settings suites and static production-leak guards run with:

```sh
bash scripts/test-macos-settings.sh --fixtures-only
pnpm run check:frontend
pnpm run check:runtime-package
cargo test --manifest-path src-tauri/Cargo.toml
pnpm run b4push
```

The [retained app-only acceptance workflow](docs/architecture/verification-matrix.md#build-once-and-verify-the-retained-bundle-on-mac)
uses one fresh bundle for package, Settings and launch controls. It records
the revision, bundle identity and logs in a caller-owned directory. Each
smoke launches via LaunchServices with isolated `HOME`, `TMPDIR`,
`XDG_CONFIG_HOME` and `CCRESDOC_CONFIG` and uses nonpersistent WebViews.
The Settings probe checks synthetic schema-v2 migration, readiness, recovery
and cleanup while its foreign listener survives. The launch probe observes
bounded native controls and verifies search and watcher freshness at a
variable mount. The broader native menu, Settings and visual walkthroughs
remain release acceptance items.

### Packaging privacy

The package build never copies a live resource tree. The staging script admits
an explicit list of app routes/configuration and the generated public theme
catalog, omits `dist`, and rejects generated `claude-*`/`codex-*` detail/status
paths plus `.ccresdoc-*` transition state. It audits staged text for synthetic
fixture sentinels and checkout paths. `runtime-manifest.json` records the
admitted files, exclusions, package facts, SHA-256 tree digest, and privacy
audit; `scripts/verify-runtime-workspace.mjs` and the Linux two-launch probe
recompute those facts and reject fixture/configured-root strings in rendered
shell, 404, and HMR responses. The final macOS package script runs the same
audit against the bundle before creating any temporary fixture source.

## Prerequisites (development only)

End users need nothing beyond the `.app` bundle. To develop or build from source:

- **Rust** (stable) — `rustup install stable`
- **Tauri CLI** — `cargo install tauri-cli` or `cargo binstall tauri-cli`
- **pnpm** — used once at build time to install `app/node_modules` (incl. native `zfb` binary)

Setup/build tooling requires Node `>=24` and pnpm `10.30.3` (the exact
`packageManager` pinned in the root, `app/`, and compatibility-fixture manifests
and in `scripts/lib/toolchain-contract.mjs`).

## Develop

```bash
pnpm --dir app install --frozen-lockfile   # once — populates node_modules incl. native zfb binary
pnpm --dir app run validate:dependencies
pnpm --dir app run validate:theme-packs
pnpm --dir app run typecheck
pnpm --dir app run check:zfb
pnpm --dir app run test:run
pnpm --dir app run test:theme-packs
cargo tauri dev
```

`cargo tauri dev` resolves the native package-root carrier from
`app/node_modules/@takazudo/zfb-<platform>/zfb`, runs the Rust generator + watcher
in-process, spawns `zfb dev` on the effective settings port, and opens the Tauri
window only after semantic `/docs/` readiness. The logo on both `/` and `/docs/`
links directly to `/docs/`. Changes to the authored source are picked up live
via HMR.

To rebuild the frontend shell manually (e.g. after changing `app/pages/`):

```bash
cd app && pnpm exec zfb build
```

(or just run `pnpm b4push`.)

## Build the .app

```bash
cargo tauri build --bundles app
```

`beforeBuildCommand` performs a frozen install/build and stages only the
lockfile-reachable runtime workspace automatically. The staged workspace keeps
the direct package-root native carrier and the selected zero-plugin config,
the app-owned source-management article, and generated theme assets. It
omits build `dist`, generated resource detail/status pages, frontend test/build
tooling, non-host zfb binaries, and disabled Node-plugin dependencies. Validate
it with `pnpm run probe:runtime-package`. For macOS 27+ arm64 acceptance, use
the [retained bundle workflow](docs/architecture/verification-matrix.md#build-once-and-verify-the-retained-bundle-on-mac).
The default standalone `scripts/test-macos-package.sh` prints its temporary bundle path,
which it removes on exit. Tauri runs build hooks from the project root, and no
global `zfb` on PATH is required.
Cargo's target directory may be configured outside the repository; resolve it
with `cargo metadata` instead of assuming `src-tauri/target/`.

To build, verify, install to `/Applications/CCResDoc.app`, launch, and confirm
semantic readiness in one command:

```bash
pnpm rebuild:local-app
```

See `.claude/skills/l-build/SKILL.md` for `/l-build`, including its
same-session `SKIP_APP_BUILD=1` fast reinstall.

## Build a release artifact

On a clean macOS arm64 host, build and verify the exact local DMG/checksum pair
without uploading or publishing anything:

```bash
bash scripts/build-macos-release.sh
```

The producer derives the version and names from the synchronized release
contract, verifies the mounted ad-hoc-signed app and packaged runtime, and
stages only the contract pair under `release-artifacts/`. The project release
skill documents the separate existing-draft upload path.

## Install a release

GitHub Releases is the canonical direct download for end users. The current
artifact is Apple Silicon (`aarch64`) and requires macOS 27 or later:
`CCResDoc_<version>_aarch64.dmg` with its matching
`CCResDoc_<version>_aarch64.dmg.sha256` file.

Download both files, verify the DMG from the directory containing them, then
open the DMG and drag `CCResDoc.app` to Applications:

```bash
shasum -a 256 -c CCResDoc_<version>_aarch64.dmg.sha256
```

The current build is ad-hoc signed but not notarized. On first launch,
right-click `CCResDoc.app` and choose **Open**, or explicitly approve it in
**System Settings → Privacy & Security**. Full Developer ID signing,
notarization, and additional architectures are future improvements, not
current distribution claims. The local macOS build is a product and
architecture choice; standard public-repository GitHub-hosted runner billing
is not the reason for it.

## Project structure

```
crates/          Rust workspace crates
  ccresdoc-claude-md/   selected Claude/Codex→MDX generators + watchers (the live engine)
src-tauri/       Tauri host (main.rs, tauri.conf.json, loading page)
app/             zfb frontend project (zudo-doc consumer, port 4892)
scripts/         local build, verification, release-contract, and release-producer commands
.github/         GitHub Actions CI and guarded release-publication workflows
.claude/skills/  local build/install and release-orchestration skills
```

See per-directory CLAUDE.md files for detailed architecture notes.
The final automated and host-only acceptance matrix is documented in
[`docs/architecture/verification-matrix.md`](docs/architecture/verification-matrix.md).

## CI

GitHub Actions runs frozen published frontend dependency, type, zfb-check,
Vitest, and native-build gates alongside a separately named actual-key
Chromium browser-navigation step. Before that ten-minute step, CI strictly
installs the pinned Playwright Chromium, then runs a separate best-effort
`playwright install-deps chromium` for Ubuntu system packages; the browser step
itself still fails if Chromium cannot launch. Rust runs
`cargo fmt --check`, `cargo clippy --workspace --exclude ccresdoc`, and
`cargo test --workspace --exclude ccresdoc`. The `ccresdoc` (src-tauri) crate is
excluded because webkit2gtk is not available on ubuntu-latest; three of its
Tauri-free modules, `settings.rs`, `appearance.rs` and `runtime.rs`, and their unit tests are
still compiled and run there through the `ccresdoc-host-domain` crate.

## Before pushing

```bash
pnpm b4push
```

Runs all reliable cross-platform checks locally: frozen dependency install and
validation, strict TypeScript, `zfb check`, Vitest, the Settings Node tests,
native zfb build, cargo fmt,
clippy/tests for the pure-Rust generator, the pruned node-free runtime lifecycle,
the independent compatibility fixture, and actual-key Chromium browser
navigation. Install the browser once with
`pnpm --dir app exec playwright install chromium`. b4push excludes the
`ccresdoc` Tauri crate from clippy/test on every host, mirroring Linux CI where
webkit2gtk/gtk3 is unavailable, so a passing b4push does not cover that crate
beyond the settings, appearance and runtime-policy tests run through
`ccresdoc-host-domain`. A release
still requires the retained-bundle macOS acceptance and the real-WebView
visual checks in the verification matrix.
