# Dependencies

The standing record of what CCResDoc depends on, why each one stays, and what an
audit already settled. Read it before proposing a removal or replacement, so that
settled questions are not reopened without new evidence.

The published toolchain (`@takazudo/zfb*` 6.0.1, `@takazudo/zudo-doc` 10.0.2, and
their runtime peers) is fixed in `scripts/lib/toolchain-contract.mjs` and is not
repeated here. Change it there; the validators read nothing back from manifests.

## Scopes

| Scope | Manifest | Lockfile | Notes |
| --- | --- | --- | --- |
| Frontend app | `app/package.json` | `app/pnpm-lock.yaml` | Standalone, hoisted; parts of it are staged into the packaged runtime |
| Compatibility fixture | `compatibility/node-free-latest/package.json` | its own `pnpm-lock.yaml` | Unpatched evidence fixture, kept on the same tuple |
| Rust workspace | `Cargo.toml` (`crates/*`, `src-tauri`) | `Cargo.lock` | macOS arm64 is the only shipped target |

The repository root `package.json` declares no dependencies.

## Audit of 2026-11-11 (after the zudo-doc 10 / zfb 6 upgrade)

### Removed

- **`vite` (app devDependency).** Nothing imports it: `app/vitest.config.ts` imports
  `vitest/config`. vitest 4.1.12 resolves vite 8.3.7 on its own, so the lockfile
  keeps the same version. This only tidies the declaration; the package count does
  not change. Do not add vite back as a direct or production dependency to steer
  test resolution.

### Upgraded or consolidated

- **TOML crates moved to Tauri's stack:** `toml` 0.8 → 1 (generator) and
  `toml_edit` 0.22 → 0.25 (settings, in `src-tauri` and `ccresdoc-host-domain`).
  - No code changes were needed. All generator and settings tests pass, including
    the lossless-save tests.
  - In the shipped macOS graph this drops `toml` 0.8, `toml_edit` 0.20 and 0.22,
    `toml_datetime` 0.6, `serde_spanned` 0.6, `toml_write` and `winnow` 0.5, and
    adds `toml_edit` 0.25.
  - On Linux, `system-deps` (GTK build) still holds `toml` 0.8.
  - Both parsers now accept TOML 1.1, a superset of 1.0, so a few documents that
    used to be malformed now parse.
- **Security lockfile updates (Rust)**, all within existing semver ranges and each
  released at least two weeks before the update:
  - `plist` 1.9.0 → 1.10.3, bringing `quick-xml` 0.39.2 → 0.42.0. This fixes
    RUSTSEC-2026-0194 and RUSTSEC-2026-0195. plist 1.10 adds `base64` 0.23.
  - `anyhow` 1.0.102 → 1.0.104 (RUSTSEC-2026-0190, unsound).
  - `rustix` 1.1.4 → 1.1.6 (RUSTSEC-2026-0486, unsound).
  - `tokio` 1.52.1 → 1.54.1 (RUSTSEC-2026-0386, unsound).
- **Security lockfile update (JS), both scopes:** `hono` 4.13.3 → 4.13.17.
  - hono is reached only through `@takazudo/zfb-runtime` (`^4.12.25`) and is staged
    into the packaged runtime.
  - The update fixes GHSA-gqvv-2mrq-wpjv, GHSA-g6gw-c38x-mqfc, GHSA-crvj-82cr-hjcx,
    GHSA-hxh3-vqpv-xpqv and GHSA-5r4p-p66f-jhc7.
  - 4.13.18 was newer than the 24-hour `minimumReleaseAge` floor and was not taken.
- **`pnpm dedupe` (app):** a stale `picomatch` 4.0.5 resolution was merged into
  4.0.7.

Net effect:

| Graph | Before | After |
| --- | --- | --- |
| `Cargo.lock` packages | 491 | 490 |
| macOS arm64 normal+build crates (excluding workspace crates) | 257 | 252 |
| `app/pnpm-lock.yaml` packages | 221 | 220 |
| Compatibility fixture packages | 141 | 141 |

### Considered and kept

JS (both scopes):

| Dependency | Why it stays |
| --- | --- |
| `zod` | Required peer of zudo-doc. The compatibility fixture also imports it directly. |
| `katex` | Optional zudo-doc peer, but the MDX components reach its loader statically even with math off. |
| `@takazudo/zfb-md-wasm` | Optional zudo-doc peer used by its highlight runtime. The app's toolchain test and the fixture import it directly. |
| `@takazudo/zfb-runtime` | Imported directly by `app/src/browser-chrome/adapter.ts`. |
| zfb native carriers | The host resolves the package-root binary; these are declared so every platform's carrier is pinned. |
| `happy-dom`, `vitest`, `typescript` | Test environment, test runner and typecheck. |
| `playwright` | Browser confirmation scripts under `scripts/`. |

Rust:

| Crate | Why it stays |
| --- | --- |
| Every declaration in `ccresdoc-claude-md`, `ccresdoc-host-domain` and `src-tauri` | Each has call sites; checked by hand in this audit. |
| `thiserror` 1 | Moving our crates to 2 frees nothing: `json-patch` (via `tauri-utils`) and the GTK crates keep 1. |
| `muda` (direct) | Used directly for the application menu; Tauri also depends on it. |
| `html5ever` | Readiness parses the served HTML for island bundles. |

### Decided: stay, with a named blocker

- **`serde_yaml` 0.9 (archived upstream; pulls `unsafe-libyaml`, RUSTSEC-2024-0449
  "unmaintained").**
  - It parses skill and agent frontmatter in the generator, including Codex skill
    metadata.
  - The usual forks (`serde_yaml_ng`, `serde_norway`) still sit on a libyaml port,
    so switching would not clear the warning.
  - `saphyr` is a different API and would change YAML edge-case semantics for
    users' own files.
  - The blocker: no maintained, serde-compatible parser removes the libyaml port
    without behavior changes. Revisit when one exists or an actual advisory lands.
  - Replacing it with first-party code is out of scope: a YAML parser is far past
    the 40-line abandon rule.

### Remaining `cargo audit` warnings (upstream, not actionable here)

- `glib` 0.18.5 (unsound) and `proc-macro-error` (unmaintained) come from Tauri's
  Linux GTK stack. They are not in the macOS build.
- `unic-*` (unmaintained) come from `urlpattern` via `tauri-utils`.
- `unsafe-libyaml`: see `serde_yaml` above.

`cargo audit` reports 0 vulnerabilities. `pnpm audit` reports none in either
scope.

### Explicit non-goals

- No recurring Dependabot or Renovate schedule. Updates happen in deliberate
  audits like this one.
- No first-party rewrites of maintained, correct dependencies.
- No speculative tuple bumps outside `scripts/lib/toolchain-contract.mjs`.

## Re-running the audit

```sh
(cd app && pnpm audit && pnpm dedupe --check)
(cd compatibility/node-free-latest && pnpm audit && pnpm dedupe --check)
cargo audit
cargo tree --workspace --target aarch64-apple-darwin -d
```
