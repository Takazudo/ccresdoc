---
name: l-build
description: "Build, verify, install, launch, and confirm the current CCResDoc.app. Use when the user says 'l-build', 'build the app', 'rebuild the local app', 'install CCResDoc', or asks whether /Applications/CCResDoc.app contains the latest code. No args."
user-invocable: true
allowed-tools:
  - Bash(pnpm rebuild:local-app*)
  - Bash(SKIP_APP_BUILD=1 pnpm rebuild:local-app*)
---

# Build and install CCResDoc

Refresh the complete local app in one command:

```bash
pnpm rebuild:local-app
```

The command:

1. prints the exact Git HEAD and warns when uncommitted bytes will be included;
2. builds an app-only Tauri release bundle;
3. resolves Cargo's real target directory instead of assuming a repo-local `target/`;
4. verifies the bundle identity, executable, pruned runtime, refresh token, and native zfb carrier;
5. asks the running app to quit and waits for owned runtime teardown;
6. moves the old `/Applications/CCResDoc.app` aside before copying the new bundle;
7. verifies the installed executable hash and runtime token against the build; and
8. launches the installed app and confirms the generated `/docs/` marker on its effective loopback port.

The workflow never discovers or signals arbitrary port owners. If the authored
preferred port is occupied, the app's normal settings-driven fallback selects
the effective port and the readiness check follows that port from the app log.

## Fast reinstall

When the release bundle was already built from the intended bytes during the
same session, reuse it without recompiling:

```bash
SKIP_APP_BUILD=1 pnpm rebuild:local-app
```

Do not use the fast path merely because some bundle exists; the full build is
the default freshness guarantee.

## Report

Relay these emitted lines:

- `git HEAD: ...`
- `installed app: ...`
- `installed executable: ...`
- `executable sha256: ...`
- `runtime token: ...`
- `ready: HTTP 200 .../docs/`

Treat the command's exit status as authoritative. On installation failure the
script restores the prior `/Applications` bundle before exiting.
