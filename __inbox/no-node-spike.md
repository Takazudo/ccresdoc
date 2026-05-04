# No-Node Spike: Minimal Changes for `cargo tauri build` Without Node.js

**Sub-issue:** https://github.com/Takazudo/ccresdoc/issues/21
**Date:** 2026-05-05
**Branch:** doc-theme-restore-no-node/s7-spike

---

## Executive Summary

Fully no-node is NOT achievable today. Three upstream blockers prevent it. A
significant Node.js reduction is achievable, but the config-loader Node dependency
is architecturally blocking and requires an upstream zfb fix before S8 can proceed
on the config-load path. The esbuild and tailwind binary delivery issues are
separately blocking and require upstream treatment. This document enumerates all
gaps, recommends approaches for each, and records the upstream issues filed.

Two zfb issues were filed during this spike (see Q6 for URLs). A third issue was
not filed because the path to resolution still requires design discussion inside
zfb. The overall verdict: S8 should wait for the upstream config-loader fix before
implementing the no-node path; in the interim S8 can reduce Node coupling to a
single binary-download step.

---

## Q0: Confirm that zfb post-#168 actually achieves no-node via deno_core

### Verdict: PARTIAL. Page rendering is no-node; config loading and binary delivery are not.

PR #168 (merged as commit e550167, with hotfix bdbfbfb on top) shipped the
`EmbeddedV8RenderHost` in `crates/zfb-render/src/embedded_v8/mod.rs`. The host
embeds a `deno_core::JsRuntime` in-process and drives the workerd-shape bundle
produced by the bundler. This is genuinely Node-free on the render path: no
subprocess, no Node.js, no miniflare.

The V8 host bootstrap sequence, as confirmed by reading the source:

1. `EmbeddedV8RenderHost::new()` constructs a `deno_core::JsRuntime` with zero
   extra extensions (no deno_fetch, no deno_web). Source:
   `crates/zfb-render/src/embedded_v8/mod.rs:156-187`.

2. `bootstrap_host_shim()` runs two `execute_script` calls: first
   `WEB_POLYFILLS_SRC` (Request, Response, Headers, URL, URLSearchParams, fetch
   stub, TextEncoder, TextDecoder, atob, btoa, structuredClone, minimal crypto),
   then `HOST_GLOBALS_SHIM_SRC` (installs `globalThis.__zfb.setBundle` and
   `globalThis.__zfb.dispatch`). Source: lines 194-204.

3. `extensions.rs` defines `NODE_STUB_SPECIFIERS` covering node:fs,
   node:fs/promises, node:path, node:url, node:buffer, and node:async_hooks.
   Each resolves to a throwing-proxy stub that fails at call time but succeeds at
   import/load time. Source: `crates/zfb-render/src/embedded_v8/extensions.rs`.

4. The `BundleModuleLoader` in
   `crates/zfb-render/src/embedded_v8/module_loader.rs` refuses any specifier that
   is not the registered bundle, a `node:*` stub, or an `ext:zfb_node_stubs/*`
   helper. Bare-import specifiers that escape the bundle are hard errors.

What is still impossible without Node.js:

- `zfb.config.ts` loading. The config loader calls esbuild to bundle the TS file,
  then spawns `node config-loader.mjs` to evaluate the bundle and extract the
  JSON default export. This is explicit in
  `crates/zfb/src/config.rs:625-745`. The error message at line 710 reads:
  "zfb requires Node.js to load zfb.config.ts". Fallback to `zfb.config.json`
  bypasses the Node step entirely.

- esbuild binary. The bundler (`zfb-build`) and the config loader both require
  the esbuild CLI binary at runtime. esbuild is platform-specific native code.
  There is no Rust-native esbuild equivalent wired into zfb today.

- tailwindcss binary. The CSS pipeline requires the tailwindcss v4 standalone CLI.
  Same situation as esbuild.

Conclusion: post-#168 zfb achieves no-node for the render step only. The upstream
claim of "no-node" is accurate for the render layer, but the build pipeline still
requires Node for config loading and external binaries for bundling and CSS.

---

## Q1: How does preact / preact-render-to-string resolve at build time?

### Current mechanism

esbuild resolves `preact` and `preact-render-to-string` as bare specifiers from
the project's `node_modules`. The bundler symlinks the project's `node_modules`
directory into the shadow temp tree (see `crates/zfb/src/commands/build.rs:791`
and `--preserve-symlinks` flag at `crates/zfb-build/src/bundler.rs:1743`). With
the `link:` protocol in `app/package.json`, `@takazudo/zfb` and
`@takazudo/zfb-runtime` also land in `app/node_modules` via pnpm's symlink graph.

esbuild then walks `node_modules` and finds preact there. The bundle produced is
self-contained ESM: all preact and preact-render-to-string code is inlined by
esbuild at bundle time. The V8 host only ever sees the bundle, never bare preact
imports.

### What breaks without pnpm install

Without `pnpm install`, `app/node_modules` does not exist. The bundler calls
`detect_project_node_modules` which checks for `<project_root>/node_modules` and
returns None if absent. When None, `bundler_input.node_modules_dir` is not set,
and esbuild has no `node_modules` to walk. Bare imports for `preact`,
`preact-render-to-string`, `@takazudo/zfb`, `@takazudo/zfb-runtime`, and `hono`
(used by zfb-runtime) all fail with resolution errors.

Additionally, `check_runtime_installed` in
`crates/zfb/src/render_pipeline.rs:750-769` explicitly walks the directory tree
for `node_modules/@takazudo/zfb-runtime` and returns a hard error with a "run
pnpm install" hint if not found.

### Recommendation: vendor the preact + hono bundle as a pre-compiled dist

The right approach is to vendor the dependencies that must be available at build
time. There are two sub-classes:

Class A - Framework packages (preact, preact-render-to-string, hono): these are
consumed by esbuild during the bundle step and inlined into the output. They do
not need to be in a full node_modules tree; they can be vendored as extracted
package contents under `app/vendor/` (checked in), and esbuild can be pointed at
them via `--alias` flags or a minimal `package.json` in the vendor dir.

Class B - zfb packages (@takazudo/zfb, @takazudo/zfb-runtime): these are
consumed by esbuild to inline framework glue code. Same treatment as Class A.

Concretely, for ccresdoc: run `npm pack preact preact-render-to-string hono` and
extract each tarball's `dist/` contents (or `src/` for packages that ship source)
into `app/vendor/preact/`, `app/vendor/preact-render-to-string/`, etc. Then pass
`--alias:preact=./vendor/preact/dist/module.js` (adjusting per package.json
exports) to esbuild, or place a minimal `node_modules/` directory in the shadow
root that zfb symlinks into.

The cleaner upstream fix is for zfb to accept an explicit `vendorDir` config option
pointing to a directory containing pre-extracted packages, and to wire that into
the esbuild `--alias` or `node_modules` injection path. Without this, ccresdoc
would need to manage the alias mapping itself per-package, which is fragile.

A zfb issue was NOT filed separately for preact resolution because it is tightly
coupled to the Q2 gap (workspace bare specifiers). The two problems have the same
root cause and should be addressed in one upstream issue. See zfb issue filed under
Q2.

---

## Q2: How do @takazudo/zfb and @takazudo/zfb-runtime resolve without pnpm install?

### Current state

Both packages are wired as `link:` protocol entries in `app/package.json`:

    "@takazudo/zfb": "link:../../zfb/packages/zfb",
    "@takazudo/zfb-runtime": "link:../../zfb/packages/zfb-runtime"

These resolve to `$HOME/repos/myoss/zfb/packages/zfb` and
`$HOME/repos/myoss/zfb/packages/zfb-runtime` on the developer's machine. After
`pnpm install`, pnpm creates symlinks in `app/node_modules/@takazudo/zfb` and
`app/node_modules/@takazudo/zfb-runtime` pointing at those absolute paths.

### What breaks without pnpm install

Without `pnpm install`, the symlinks do not exist. The `check_runtime_installed`
guard mentioned in Q1 fires first and halts the build. If that check were bypassed,
esbuild would next fail to resolve `@takazudo/zfb-runtime` because there is no
`node_modules` directory for it to walk.

### Can zfb's V8 host resolve workspace bare specifiers from an absolute path?

No. The V8 host only ever sees the already-bundled ESM output. The resolution
problem is at esbuild invocation time, not inside the V8 host. The V8 host's
`BundleModuleLoader` refuses any specifier that is not the registered bundle or a
`node:*` stub. This is correct design: by the time the host sees the bundle, all
bare imports have been inlined by esbuild.

esbuild itself does have `--alias` and path-alias support, but it requires the
aliased path to point to a real file. Without a `node_modules` directory, or
without explicit `--alias:@takazudo/zfb=<absolute-path-to-pkg-src>` flags,
esbuild cannot resolve the packages.

### Recommendation and filed issue

zfb should add a mechanism to resolve `@takazudo/zfb` and `@takazudo/zfb-runtime`
from the zfb binary's own installation path, not from the consumer's
`node_modules`. Since the `zfb` binary is installed via `cargo install --path
crates/zfb`, its data directory is a natural slot for embedding the package
sources as readonly assets (using Rust's `include_dir!` macro or similar).

This is the cleanest path to no-node for Q1 and Q2 together: zfb bundles its own
TypeScript packages and resolves them from within the binary, bypassing the
`node_modules` requirement entirely for zfb-internal packages. Consumer packages
(preact, hono) would still need vendoring.

Filed as zfb issue: https://github.com/Takazudo/zudo-front-builder/issues/183

---

## Q3: How does the esbuild standalone binary ship to consumer projects?

### Current state

esbuild is the BIG blocker. The build pipeline uses esbuild in three places:

1. Config loading: `crates/zfb/src/config.rs` calls esbuild to bundle
   `zfb.config.ts` before running node on the output.

2. Bundle step: `crates/zfb-build/src/bundler.rs` calls esbuild to produce the
   worker ESM bundle from user pages + components + zfb-runtime.

3. Islands bundle: `crates/zfb-islands/src/esbuild.rs` calls esbuild for the
   client-side islands bundle.

The binary resolution order (same for all three, from `esbuild.rs:213-237`):
   1. Explicit override via `BundlerInput::esbuild_binary`.
   2. `ZFB_ESBUILD_BIN` environment variable.
   3. `crates/zfb/binaries/esbuild/esbuild` relative to CWD.

The `crates/zfb/binaries/README.md` documents that no binary is committed; the
slot is reserved but not populated. The release-tarball assembly epic is not yet
complete (referenced as "issue #5" in the README, but that issue is a different
topic). There is no automatic download; the operator must provide the binary.

For the worktree, `app/.env.build` hard-codes the esbuild binary path inside the
zfb checkout's pnpm-installed `node_modules`:

    ZFB_ESBUILD_BIN="$HOME/repos/myoss/zfb/node_modules/.pnpm/@esbuild+darwin-arm64@0.25.12/..."

This is a pnpm-installed path and requires pnpm install to exist.

### Options

Option A: cargo install vendors esbuild into the binary's data directory.
At `cargo install --path crates/zfb` time, a build script downloads the platform-
appropriate esbuild tarball from esbuild's GitHub releases, verifies the SHA-256,
and embeds it into the binary or places it adjacent to the installed binary. At
runtime, zfb locates esbuild relative to its own executable path (std::env::current_exe).
This is analogous to how cargo-tauri ships bundled tools.

Option B: ccresdoc downloads esbuild via a build script. A pre-build hook in
`src-tauri/build.rs` or a shell script downloads the pinned esbuild tarball for
the current platform, verifies its hash, and places it at a known path that is
then set via `ZFB_ESBUILD_BIN`. This keeps the download logic in ccresdoc, not
upstream.

Option C: zfb auto-downloads on first run if the binary slot is empty. The zfb
CLI detects that the slot is empty at startup and downloads the pinned binary.
Similar to rustup's toolchain download behavior.

Option A is the cleanest for consumers and is the documented intent of the
`crates/zfb/binaries/` slot (per the README). It requires upstream zfb work.
Option B avoids upstream dependency but is fragile: it must be maintained per
platform per ccresdoc and duplicates logic that zfb should own. Option C has
the same implementation as A but the download happens lazily.

Recommendation: Option A (upstream zfb change). Filed as:

zfb issue: https://github.com/Takazudo/zudo-front-builder/issues/184

Until that issue lands, ccresdoc can use Option B (download script) as a
temporary workaround, but S8 should wait for the upstream fix before committing
to an Option-B architecture.

Note: tailwindcss has a download script (`pnpm fetch:tailwind`) in the zfb repo,
and the binary at `crates/zfb/binaries/tailwindcss-v4` is the result. The same
pattern needs to apply to esbuild and both need to be bundled into the `cargo
install` step. See also issue #186 for tailwindcss-specific tracking.

---

## Q4: How does Tauri beforeBuildCommand invoke zfb build without pnpm?

### Current state

`src-tauri/tauri.conf.json` has:

    "beforeBuildCommand": "pnpm --filter app build"

This runs the `build` script in `app/package.json`:

    ZFB_ESBUILD_BIN="..." ZFB_TAILWIND_BIN="..." zfb build

Two issues:

1. `pnpm --filter app build` requires pnpm to be installed and the workspace to
   have been set up.

2. Even if pnpm is available, `pnpm build` requires `app/node_modules` to be
   populated (for preact, @takazudo/zfb, etc.), which requires `pnpm install`.

### What needs to change

The `beforeBuildCommand` must be changed to invoke `zfb build` directly from
`app/`, bypassing pnpm entirely. Something like:

    "beforeBuildCommand": "ZFB_ESBUILD_BIN=/path/to/esbuild ZFB_TAILWIND_BIN=/path/to/tailwind zfb build --cwd ../app"

Or, if zfb adds a `--project-dir` flag:

    "beforeBuildCommand": "zfb build --project-dir ../app"

The working directory matters because zfb resolves config relative to CWD.

Separately, if zfb gains the ability to locate esbuild and tailwind from its own
binary path (Q3 resolution), the `ZFB_*_BIN` env vars become optional.

Does zfb require a `package.json` in the project directory? Reading
`crates/zfb/src/commands/build.rs:78-91`, the only mandatory check is that
`pages/` exists relative to CWD. The `package.json` is not read by zfb directly;
it is only consulted by pnpm for workspace wiring. If zfb can resolve its
dependencies without `node_modules` (Q1/Q2 resolution), a `package.json` becomes
unnecessary.

With a stripped `package.json` (or none), `zfb build` should work if:
- zfb binary is in PATH
- esbuild binary is findable (ZFB_ESBUILD_BIN or bundled)
- tailwindcss binary is findable (ZFB_TAILWIND_BIN or bundled)
- zfb.config.ts is reachable as zfb.config.json (bypassing Node config loader)
  OR the config loader is fixed to run without Node

The zfb.config.ts in ccresdoc is simple (framework: "preact", tailwind enabled,
one plugin). It can be rewritten as `zfb.config.json` without loss of
functionality, as long as the plugin path resolution also works for JSON configs.

Reading `config.rs:493-507`, JSON config is parsed via serde_json and validates
correctly. The only concern is plugin name resolution: in the JSON path, plugins
are stored as-is, and the resolved_module field is expected to be set. Whether
the JSON path resolves plugin module specifiers needs verification.

---

## Q5: New developer onboarding flow in the no-node world

### Proposed README delta

The no-node onboarding flow for ccresdoc, once all upstream fixes land:

    Prerequisites:
      - Rust stable + cargo
      - zfb installed: cargo install --path <path-to-zfb-checkout>/crates/zfb
        (this also downloads and stages the esbuild and tailwindcss binaries)

    Build:
      cargo tauri build
      (tauri's beforeBuildCommand invokes zfb build directly, no pnpm needed)

    Development:
      cargo tauri dev
      (tauri's beforeDevCommand starts zfb dev, serving from app/)

The critical change is that `zfb` becomes a self-contained build tool that carries
its own bundler (esbuild) and CSS processor (tailwindcss). The consumer project
needs no `package.json`, no `node_modules`, and no pnpm. The zfb binary is the
single tool that needs to be in PATH.

This matches the zfb README's stated goal ("install one Rust binary, run zfb
build") but the implementation is not yet complete. The README delta is the target
state for post-S8 documentation.

### Residual blockers before this onboarding works

1. Config loader must not require Node (see Q0 and Q2).
2. esbuild binary must ship with `cargo install zfb` (see Q3, issue #184).
3. `@takazudo/zfb` and `@takazudo/zfb-runtime` must be resolvable without
   `node_modules` (see Q2, issue #183).
4. `check_runtime_installed` must not hard-error when node_modules is absent
   (will be fixed as part of issue #183).
5. tailwindcss binary delivery (issue #186): currently requires `pnpm fetch:tailwind`;
   bundling it into `cargo install` follows the same pattern as esbuild (#184).

---

## Q6: Upstream zfb issues filed during this spike

### Issue 1: @takazudo/zfb and @takazudo/zfb-runtime should resolve from the zfb binary installation, not consumer node_modules

URL: https://github.com/Takazudo/zudo-front-builder/issues/183

Filed from: /home/takazudo/repos/myoss/zfb working directory.

Gap: When a consumer project has no `node_modules` (no-node scenario), esbuild
cannot resolve `@takazudo/zfb` and `@takazudo/zfb-runtime`. The
`check_runtime_installed` guard in `crates/zfb/src/render_pipeline.rs:750-769`
also hard-errors with a "run pnpm install" message. Both zfb-internal packages
are owned by zfb itself; they should not require a consumer-side pnpm install.

Minimal reproducer: Create a fresh directory with a `pages/index.tsx` and a
`zfb.config.json` (not .ts). Run `zfb build`. The build fails with "could not
find node_modules/@takazudo/zfb-runtime".

Why ccresdoc cannot work around it: ccresdoc could theoretically symlink the zfb
checkout's packages into app/node_modules manually, but this is machine-specific
and breaks on any machine where zfb is installed via cargo install rather than a
local checkout.

Reference to https://github.com/Takazudo/ccresdoc/issues/21.

### Issue 2: esbuild standalone binary should ship with cargo install zfb

URL: https://github.com/Takazudo/zudo-front-builder/issues/184

Filed from: /home/takazudo/repos/myoss/zfb working directory.

Gap: esbuild is a mandatory runtime dependency of `zfb build` and `zfb dev`. It
is a platform-specific native binary that cannot be resolved from the consumer's
`node_modules` in a no-node scenario. The `crates/zfb/binaries/esbuild/esbuild`
slot is reserved but never populated by `cargo install`. A consumer who has no
pnpm/npm has no way to obtain the esbuild binary automatically.

Minimal reproducer: Install zfb via `cargo install --path crates/zfb`. Observe
that `crates/zfb/binaries/esbuild/esbuild` is empty. Run `zfb build` in any
project without setting `ZFB_ESBUILD_BIN`. The build fails with "esbuild binary
not found at default slot".

Why ccresdoc cannot work around it without zfb-side changes: ccresdoc could
download esbuild itself in a pre-build script, but this duplicates version pinning
logic that zfb owns (the pinned version `EXPECTED_ESBUILD_VERSION = "0.25.12"` is
in `crates/zfb-islands/src/esbuild.rs`). If zfb bumps the esbuild pin, ccresdoc's
download script would be out of sync.

Reference to https://github.com/Takazudo/ccresdoc/issues/21 (this sub-issue).

### Issue NOT filed: Node.js requirement for config-loader

The config-loader Node dependency (`crates/zfb/src/config.rs:625-745`) was not
filed as a separate issue because:

1. It is mentioned in the existing code as a known limitation: the error message
   at line 710 says "or point zfb at a node binary by setting the ZFB_NODE_BIN
   env var on a future zfb release", indicating it is already on the roadmap.

2. ccresdoc can work around it TODAY by converting `app/zfb.config.ts` to
   `app/zfb.config.json`. The JSON config path in zfb does not invoke Node. The
   ccresdoc config is simple enough (framework, tailwind, one plugin) that the
   JSON form is straightforward.

   The plugin resolution for JSON configs needs verification, but if it works,
   this unblocks the config-loading Node dependency for ccresdoc specifically
   without upstream changes.

---

## Lessons from zudo-doc2

The zudo-doc2 lessons file at
`/home/takazudo/repos/myoss/zudo-doc2/.claude/skills/l-lessons-zfb-migration-parity/SKILL.md`
(entry 2026-05-05, "zfb pin bump to embed-v8, epic #1407") documents the key
finding that drove this spike: PR #168's embedded V8 host replaced the miniflare
subprocess for rendering, but consumer-side keyword grep (`Backend::`,
`miniflare`, `workerd`) found no consumer changes required because the
architecture change was encapsulated. However, the same lesson also warns that
"runtime-shape changes" (page handler call shape, plugin contract shape) can break
at runtime despite no compile-time signal.

For the no-node investigation, the parallel lesson is: the V8 host is no-node for
the render step, but esbuild and node are still required earlier in the pipeline.
A keyword grep for "miniflare" would have found zero hits (correct) while the
config-loader Node dependency (`node config-loader.mjs`) would have been missed.

---

## Summary: What is achievable today vs. what requires upstream fixes

### Achievable today without upstream changes

- Convert `app/zfb.config.ts` to `app/zfb.config.json` to remove the Node.js
  dependency for config loading. The render and bundle pipeline is already
  Node-free after PR #168 (bdbfbfb).

- The `beforeBuildCommand` in `tauri.conf.json` can be changed to run
  `zfb build` directly (once esbuild/tailwind binary delivery and package
  resolution are fixed).

### Requires upstream zfb changes before S8 can complete

1. Issue #184: esbuild binary must ship with `cargo install zfb` (BLOCKING for
   any developer without a local zfb pnpm install).

2. Issue #183: `@takazudo/zfb` and `@takazudo/zfb-runtime` must resolve from the
   zfb binary, not from consumer `node_modules` (BLOCKING for clean no-node).

3. Issue #186: tailwindcss binary delivery: currently requires `pnpm fetch:tailwind`
   from the zfb repo. The same bundling-into-cargo-install treatment as esbuild.

### Recommended path for S8 (post-upstream-fixes)

1. Convert `app/zfb.config.ts` to `app/zfb.config.json`.
2. Remove `app/package.json` link: entries for @takazudo/zfb packages (once #183
   lands these are resolved by zfb itself).
3. Retain a minimal `app/package.json` for preact and preact-render-to-string
   until zfb adds vendor support, OR vendor those packages directly.
4. Change `beforeBuildCommand` in `tauri.conf.json` to `zfb build --cwd ../app`
   (or equivalent flag, once zfb supports it).
5. Update onboarding: `cargo install zfb` (which downloads esbuild + tailwindcss)
   then `cargo tauri build`.

### If "fully no-node not feasible today": residual blockers

Fully no-node is not feasible today. The residual blockers (all requiring upstream
zfb work) are:

- esbuild binary delivery (#184): no binary, no bundling.
- @takazudo/zfb-runtime resolution (#183): no pnpm, no resolution.
- tailwindcss binary delivery (#186): no binary, no CSS.

S8's scope should be adjusted to: implement the changes achievable without
upstream fixes (zfb.config.json conversion, beforeBuildCommand cleanup), and
create placeholder issues in ccresdoc that are blocked on zfb #183, #184,
and #186.

---

## Filed upstream issue URLs

- https://github.com/Takazudo/zudo-front-builder/issues/183 (zfb package resolution without node_modules)
- https://github.com/Takazudo/zudo-front-builder/issues/184 (esbuild binary delivery via cargo install)
- https://github.com/Takazudo/zudo-front-builder/issues/186 (tailwindcss binary delivery via cargo install)
