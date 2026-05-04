# CCResDoc

A macOS documentation viewer for `$HOME/.claude/` — renders Markdown files, CLAUDE.md hierarchies, skills, and commands as a browsable local web app inside a native Tauri window.

The app bundles an embedded axum HTTP server that serves a zfb-built static shell and renders Markdown content dynamically. No Node.js or external dependencies are required to run the finished `.app`.

## Prerequisites (development only)

End users need nothing beyond the `.app` bundle. To develop or build from source:

- **Rust** (stable) — `rustup install stable`
- **Tauri CLI** — `cargo install tauri-cli` or `cargo binstall tauri-cli`
- **pnpm** — `npm install -g pnpm`
- **Node.js** (LTS) — for running `pnpm install` and the zfb build
- **zfb binary** — `cd $HOME/repos/myoss/zfb && cargo install --path crates/zfb`
- The `zfb` local package links assume the zfb repo lives at `$HOME/repos/myoss/zfb`

## Develop

```
pnpm install
cargo tauri dev
```

`cargo tauri dev` starts the embedded server and opens the Tauri window pointing at `http://localhost:4892/`. Changes to `app/` require a manual `pnpm --filter app build` to take effect in dev mode.

## Build the .app

```
cargo tauri build
```

This runs `pnpm --filter app build` automatically (via `beforeBuildCommand`), then compiles and bundles the Tauri app. The output is at `src-tauri/target/release/bundle/macos/CCResDoc.app`.

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

GitHub Actions runs `cargo fmt --check`, `cargo clippy --workspace --all-targets`, and `cargo test --workspace` on every push and PR. The `pnpm --filter app build` step is run locally (via `pnpm b4push`) but deferred from CI until zfb is published to npm — see `.github/workflows/ci.yml` for the rationale.

## Before pushing

```
pnpm b4push
```

Runs all four checks locally: cargo fmt, clippy, test, and the app build.
