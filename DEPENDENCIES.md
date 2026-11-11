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

Nothing. Every declaration in both pnpm scopes and all three Cargo manifests has
call sites or a recorded reason below. The two candidates that were tried were
reverted after review (see "Tried and reverted").

### Updated for security (lockfile only)

- **Rust**, all within existing semver ranges and each released at least two
  weeks before the update:
  - `plist` 1.9.0 → 1.10.3, bringing `quick-xml` 0.39.2 → 0.42.0. This fixes
    RUSTSEC-2026-0194 and RUSTSEC-2026-0195. plist 1.10 adds `base64` 0.23.
  - `anyhow` 1.0.102 → 1.0.104 (RUSTSEC-2026-0190, unsound).
  - `rustix` 1.1.4 → 1.1.6 (RUSTSEC-2026-0486, unsound).
  - `tokio` 1.52.1 → 1.54.1 (RUSTSEC-2026-0386, unsound).
- **JS, both scopes:** `hono` 4.13.3 → 4.13.17.
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
| `Cargo.lock` packages | 491 | 492 (plist's new `base64` 0.23) |
| `app/pnpm-lock.yaml` packages | 221 | 220 |
| Compatibility fixture packages | 141 | 141 |

### Tried and reverted (settled; do not re-propose without new evidence)

- **`toml` 0.8 → 1 and `toml_edit` 0.22 → 0.25** (to match Tauri's stack). It
  compiled with no code changes and would have dropped 7 crates from the macOS
  graph. It was reverted because both newer crates parse TOML 1.1:
  - A schema-v1 config that is malformed under TOML 1.0 must be left untouched.
    Under 1.1 it could instead be recognized and retired at startup.
  - Ordinary Save would rewrite v2 documents that are currently `Malformed` and
    need explicit replacement.
  - The generator would show Codex configs as valid that a TOML 1.0 consumer
    rejects.

  Revisit only together with tests that pin the malformed-file boundary under the
  new parser.
- **Dropping the `vite` dev pin.** Nothing imports vite directly; vitest resolves
  it. Removing the declaration freed no packages, and it had two side effects.
  It took vite out of `APP_DEV_DEPENDENCIES`, so the validators no longer check
  its version. It also took vite out of the staged-runtime exclusion list, which
  `scripts/verify-runtime-workspace.mjs` derives from the manifest's
  devDependencies. Keep it as a dev-only pin, never a production dependency.

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
| `vite` | Dev-only pin for test resolution; see "Tried and reverted". |
| `playwright` | Browser confirmation scripts under `scripts/`. |

Rust:

| Crate | Why it stays |
| --- | --- |
| Every declaration in `ccresdoc-claude-md`, `ccresdoc-host-domain` and `src-tauri` | Each has call sites; checked by hand in this audit. |
| `thiserror` 1 | Moving our crates to 2 frees nothing: `json-patch` (via `tauri-utils`) and the GTK crates keep 1. |
| `muda` (direct) | `browser_bridge.rs` parses shortcut accelerators with muda's own parser, so a binding Tauri would silently drop is rejected first. Tauri also depends on it. |
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
  - Replacing it with first-party code is out of scope: a YAML parser is not a
    small utility to hand-roll.

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
