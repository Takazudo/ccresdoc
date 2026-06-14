#!/usr/bin/env bash
# check-zfb-pin.sh — assert all @takazudo/zfb* entries in app/package.json
# share a single pinned version. Exits non-zero if any version drifts.
#
# Intent: app/package.json pins @takazudo/zfb, @takazudo/zfb-adapter-cloudflare,
# @takazudo/zfb-runtime, and the platform binaries (@takazudo/zfb-<platform>) to
# the same semver. These must move in lockstep (they are released together). There
# is no single-source mechanism at JSON level — this script is the enforcement gate.
#
# Wire this into scripts/run-b4push.sh to catch drift before push.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PKG="$ROOT_DIR/app/package.json"

# Extract all @takazudo/zfb* version strings (exact pins, not ranges)
versions=$(python3 - <<'PY'
import json, sys
with open(sys.argv[1]) as f:
    pkg = json.load(f)
deps = {}
for section in ("dependencies", "optionalDependencies", "devDependencies"):
    deps.update(pkg.get(section, {}))
zfb_vers = sorted(set(
    v for k, v in deps.items()
    if k.startswith("@takazudo/zfb")
))
print("\n".join(zfb_vers))
PY
"$PKG")

count=$(echo "$versions" | grep -c .)

if [ "$count" -eq 0 ]; then
  echo "check-zfb-pin: no @takazudo/zfb* entries found in app/package.json"
  exit 1
fi

if [ "$count" -gt 1 ]; then
  echo "check-zfb-pin: FAIL — @takazudo/zfb* versions are not aligned:"
  echo "$versions" | sed 's/^/  /'
  echo "All @takazudo/zfb* packages must share a single pinned version."
  exit 1
fi

echo "check-zfb-pin: OK — all @takazudo/zfb* packages pinned to $(echo "$versions" | head -1)"
