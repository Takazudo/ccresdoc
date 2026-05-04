# src-tauri/ — CCResDoc Tauri Wrapper

Tauri v2 macOS app that opens the CCResDoc doc viewer in a native window. The HTTP server is embedded directly in the Tauri binary — no Node.js, no sidecar process.

## Quick start

```
# Dev mode
cargo tauri dev

# Production build
cargo tauri build
```

## Architecture

The embedded axum server (`ccresdoc-server`) runs on a dedicated tokio runtime thread spawned during Tauri's `setup` phase. It binds on port 4892 and serves until the process exits.

```
main() ──► kill_port(4892)          clean up stale processes
        ──► setup()
              ├─ start_embedded_server(dist_dir)   tokio thread, runs forever
              ├─ build menu
              └─ open window
                   IS_DEV: navigate directly to http://localhost:4892/
                   PROD:   show loading page → poll /___ready → navigate
```

No node detection, no sidecar spawn, no process group management. The server is always co-located with the Tauri binary.

## Key files

- `src/main.rs` — all Rust code: server startup, menus, window management, zoom
- `frontend/index.html` — loading page shown in production while server starts
- `capabilities/default.json` — allows WebView to access localhost:4892
- `tauri.conf.json` — Tauri configuration (port, bundle resources, beforeBuildCommand)

## How dist_dir is resolved

| Context | Path |
| --- | --- |
| Dev (`cargo tauri dev`) | `$HOME/.claude/app/dist` — locally built output |
| Production bundle | `Contents/Resources/_up_/app/dist` |

Tauri places resources with `..` traversal under `_up_/` inside `Contents/Resources/`. The `_up_` segment is a Tauri convention for the parent-directory step in a resource path that starts with `../`.

## Production startup sequence

1. `kill_port(4892)` — SIGTERM any process already on the port
2. `start_embedded_server(dist_dir)` — spawn tokio thread, bind axum
3. Open window with bundled `frontend/index.html` (loading page)
4. Background thread polls `GET /___ready` up to 30s
5. On 200: `w.navigate("http://localhost:4892/")` — switches to the docs
6. On timeout: emit `launch-error` event → loading page shows error panel with retry button

## Dev startup

`cargo tauri dev` sets `debug_assertions = true` (the `IS_DEV` constant). In dev mode:

- `kill_port` is skipped (avoids killing the dev server you may already have running)
- Window opens directly with `WebviewUrl::External("http://localhost:4892/")` — no loading page
- The server still starts on the tokio thread; Tauri's `devUrl` polling waits for it

## Menu actions

| Menu item | Shortcut | Behaviour |
| --- | --- | --- |
| Refresh | Cmd+R | Re-navigates window to docs URL |
| Toggle Developer Tools | Cmd+Alt+I | Opens/closes WebKit devtools |
| Actual Size | Cmd+0 | Resets zoom to 1.0 |
| Zoom In | Cmd+= | Zoom +0.1 (max 3.0) |
| Zoom Out | Cmd+- | Zoom -0.1 (min 0.1) |

## Testing the launch script

```
APP_OVERRIDE="/Applications/CCResDoc.app" bash scripts/test-launch.sh 3
```

## Anti-white-flash pattern

The window is shown with the bundled loading page first, then navigated once the server is ready. This avoids both a white flash and a visible URL bar flicker.

## Platform

macOS arm64 only. The `cargo tauri build` CI step is deferred from the GitHub Actions workflow — see `.github/workflows/ci.yml` for details.
