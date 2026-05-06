# CCResDoc

A macOS documentation viewer for `$HOME/.claude/` — renders Markdown files, CLAUDE.md hierarchies, skills, and commands as a browsable local web app inside a native Tauri window.

The app bundles an embedded axum HTTP server that serves a zfb-built static shell and renders Markdown content dynamically. No Node.js or external dependencies are required to run the finished `.app`.

## Prerequisites (development only)

End users need nothing beyond the `.app` bundle. To develop or build from source:

- **Rust** (stable) — `rustup install stable`
- **Tauri CLI** — `cargo install tauri-cli` or `cargo binstall tauri-cli`
- **zfb binary** — `cargo install --path $HOME/repos/myoss/zfb/crates/zfb` — `cargo install` downloads esbuild + tailwindcss-v4 standalone binaries to the zfb source tree

ccresdoc's build invocations (`tauri.conf.json` `beforeBuildCommand` and `scripts/run-b4push.sh`) point at those binaries via `ZFB_ESBUILD_BIN` and `ZFB_TAILWIND_BIN` env vars. This is a temporary workaround until zfb extracts those binaries from its `include_dir!` snapshot at runtime; once that lands upstream, the env-var prefixes can be dropped. See `app/CLAUDE.md` for the full list of known zfb feature gaps.

## Develop

```
cargo tauri dev
```

`cargo tauri dev` starts the embedded server and opens the Tauri window pointing at `http://localhost:4892/`. Changes to `app/` require a manual rebuild to take effect in dev mode:

```
ZFB_ESBUILD_BIN=$HOME/repos/myoss/zfb/crates/zfb/binaries/esbuild/esbuild \
ZFB_TAILWIND_BIN=$HOME/repos/myoss/zfb/crates/zfb/binaries/tailwindcss-v4 \
  zfb build --cwd app
```

(or just run `bash scripts/run-b4push.sh` which sets the env vars itself.)

## Build the .app

```
cargo tauri build
```

This runs the env-var-prefixed `zfb build` automatically (via `beforeBuildCommand` in `src-tauri/tauri.conf.json`), then compiles and bundles the Tauri app. The output is at `src-tauri/target/release/bundle/macos/CCResDoc.app`.

## Project structure

```
crates/          Rust library crates (resources, renderer, server)
src-tauri/       Tauri wrapper (main.rs, tauri.conf.json)
app/             zfb frontend project (TypeScript/Preact static shell)
scripts/         run-b4push.sh, test-launch.sh
.github/         GitHub Actions CI workflow
```

See per-directory CLAUDE.md files for detailed architecture notes.

## CI

GitHub Actions runs `cargo fmt --check`, `cargo clippy --workspace --all-targets`, and `cargo test --workspace` on every push and PR. The `zfb build` step is run locally (via `scripts/run-b4push.sh`) but deferred from CI — see `.github/workflows/ci.yml` for the rationale.

## Before pushing

```
bash scripts/run-b4push.sh
```

Runs all four checks locally: cargo fmt, clippy, test, and the app build.
