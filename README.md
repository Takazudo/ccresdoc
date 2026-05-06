# CCResDoc

A macOS documentation viewer for `$HOME/.claude/` — renders Markdown files, CLAUDE.md hierarchies, skills, and commands as a browsable local web app inside a native Tauri window.

The app bundles an embedded axum HTTP server that serves a zfb-built static shell and renders Markdown content dynamically. No Node.js or external dependencies are required to run the finished `.app`.

## Prerequisites (development only)

End users need nothing beyond the `.app` bundle. To develop or build from source:

- **Rust** (stable) — `rustup install stable`
- **Tauri CLI** — `cargo install tauri-cli` or `cargo binstall tauri-cli`
- **zfb binary** — `cargo install --path $HOME/repos/myoss/zfb/crates/zfb` — auto-downloads esbuild + tailwind binaries at install time; no Node.js or pnpm required

## Develop

```
cargo tauri dev
```

`cargo tauri dev` starts the embedded server and opens the Tauri window pointing at `http://localhost:4892/`. Changes to `app/` require a manual `cd app && zfb build` to take effect in dev mode.

## Build the .app

```
cargo tauri build
```

This runs `cd ../app && zfb build` automatically (via `beforeBuildCommand`), then compiles and bundles the Tauri app. The output is at `src-tauri/target/release/bundle/macos/CCResDoc.app`.

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
