# Latest zudo-doc/zfb verification matrix

CCResDoc consumes `@takazudo/zfb`/runtime/MD-WASM `2.7.1` and
`@takazudo/zudo-doc` `5.6.0`. The application keeps host-owned routes and the
in-process Rust resource generator, while the composed zfb configuration ends
with `plugins: []`. Node and pnpm are setup/build tools only; the staged
runtime resolves the platform `zfb` executable directly.

## Automated gates

Run the full local gate with:

```sh
pnpm run b4push
```

It proves the frozen dependency graph and installed tree, strict TypeScript,
`zfb check`, Vitest route/Markdown/navigation contracts, native production
build, Rust formatting/lints/tests (including generate, watch, live smoke, and
unchanged-write coverage), the pruned runtime workspace, and the frozen
compatibility fixture. The staged-runtime probe renders `/` and `/docs/`,
checks missing-route status and hydration assets, observes a content reload
event, samples the process group repeatedly, starts twice, and fails if Node or
`plugin-host.mjs` appears. The compatibility fixture independently records why
the package presets with plugins are rejected and proves the selected
zero-plugin configuration.

Linux CI runs the same frontend, runtime, compatibility, and pure-Rust gates.
The Tauri crate is excluded from Linux clippy/test because its WebKit/GTK
system libraries are unavailable on the runner; this is an environment limit,
not a passed host check.

## Host-only release gates

Packaged support is macOS arm64. Before a release, run on that host:

```sh
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/test-macos-package.sh
```

The package script builds and launches `CCResDoc.app`, checks the direct
`@takazudo/zfb-darwin-arm64/zfb` path and executable mode, samples processes,
and verifies clean teardown. This Linux implementation session did not run
that gate and does not claim it passed.

Real-WebView/browser visual acceptance also remains manual: 1440x900 and
390x844, light/dark, visible focus rings, overflow, mobile drawer and inert
background, theme toggle, hydrated controls, and soft navigation. Unit tests
cover the static hydration/client-navigation and keyboard/mobile contracts,
but they are not a substitute for that visual pass.
