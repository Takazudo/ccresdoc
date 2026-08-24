#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TMP_BASE="${TMPDIR:-/tmp}"
TMP_BASE="${TMP_BASE%/}"
WORK_DIR=""
MOUNT_POINT=""
DMG_ATTACHED=0
PAIR_STAGING_STARTED=0
PAIR_COMPLETE=0
UPLOAD_TAG=""
CLOBBER=0
BUNDLE_ID="com.takazudo.ccresdoc"
PACKAGE_FACTS="$REPO_ROOT/compatibility/node-free-latest/evidence/package-facts.json"

usage() {
  cat >&2 <<'EOF'
Usage:
  bash scripts/build-macos-release.sh
  bash scripts/build-macos-release.sh --upload v<stable-semver> [--clobber]

The default builds and verifies the release without uploading. Upload mode only
updates an existing matching draft Release; it never creates or publishes one.
EOF
}

fail() {
  echo "build-macos-release.sh: $*" >&2
  exit 1
}

cleanup() {
  local status=$?
  trap - EXIT INT TERM
  if [[ "$DMG_ATTACHED" == "1" && -n "$MOUNT_POINT" ]]; then
    if ! hdiutil detach "$MOUNT_POINT" >/dev/null 2>&1; then
      hdiutil detach -force "$MOUNT_POINT" >/dev/null 2>&1 || {
        echo "build-macos-release.sh: failed to detach $MOUNT_POINT" >&2
        status=1
      }
    fi
    DMG_ATTACHED=0
  fi
  if [[ "$PAIR_STAGING_STARTED" == "1" && "$PAIR_COMPLETE" != "1" ]]; then
    [[ -n "${ARTIFACT_PATH:-}" ]] && rm -f "$ARTIFACT_PATH"
    [[ -n "${CHECKSUM_PATH:-}" ]] && rm -f "$CHECKSUM_PATH"
  fi
  if [[ -n "$WORK_DIR" ]]; then
    case "$WORK_DIR" in
      "$TMP_BASE"/ccresdoc-macos-release.*) rm -rf "$WORK_DIR" ;;
      *)
        echo "build-macos-release.sh: refusing to remove unexpected temporary path: $WORK_DIR" >&2
        status=1
        ;;
    esac
  fi
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

while (( $# > 0 )); do
  case "$1" in
    --upload)
      (( $# >= 2 )) || { usage; exit 2; }
      [[ -z "$UPLOAD_TAG" ]] || { echo "--upload may be supplied only once" >&2; exit 2; }
      UPLOAD_TAG="$2"
      shift 2
      ;;
    --clobber)
      CLOBBER=1
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage
      exit 2
      ;;
  esac
done

[[ "$CLOBBER" == "0" || -n "$UPLOAD_TAG" ]] || {
  echo "--clobber is allowed only with --upload" >&2
  exit 2
}

[[ "$(uname -s)" == "Darwin" ]] || fail "requires macOS (Darwin)"
[[ "$(uname -m)" == "arm64" ]] || fail "requires an Apple-silicon arm64 host"

REQUIRED_TOOLS=(cargo pnpm node hdiutil codesign lipo shasum git find readlink stat open curl lsof osascript)
if [[ -n "$UPLOAD_TAG" ]]; then REQUIRED_TOOLS+=(gh); fi
for tool in "${REQUIRED_TOOLS[@]}"; do
  command -v "$tool" >/dev/null 2>&1 || fail "required tool not found: $tool"
done
[[ -x /usr/libexec/PlistBuddy ]] || fail "required tool not found: /usr/libexec/PlistBuddy"
[[ -f "$PACKAGE_FACTS" ]] || fail "package facts not found: $PACKAGE_FACTS"

CONTRACT_JSON="$(node "$SCRIPT_DIR/release-contract.mjs" check --root "$REPO_ROOT" --json)" ||
  fail "release contract is missing, malformed, or unsynchronized"
read_contract_field() {
  node -e 'const value=JSON.parse(process.argv[1])[process.argv[2]]; if (typeof value !== "string" || !value) process.exit(1); process.stdout.write(value)' "$CONTRACT_JSON" "$1"
}
VERSION="$(read_contract_field version)" || fail "release contract has no version"
EXPECTED_TAG="$(read_contract_field tag)" || fail "release contract has no tag"
ARTIFACT_NAME="$(read_contract_field artifactName)" || fail "release contract has no artifact name"
CHECKSUM_NAME="$(read_contract_field checksumName)" || fail "release contract has no checksum name"
ARTIFACT_RELATIVE_PATH="$(read_contract_field artifactPath)" || fail "release contract has no artifact path"
CHECKSUM_RELATIVE_PATH="$(read_contract_field checksumPath)" || fail "release contract has no checksum path"
ARTIFACT_PATH="$REPO_ROOT/$ARTIFACT_RELATIVE_PATH"
CHECKSUM_PATH="$REPO_ROOT/$CHECKSUM_RELATIVE_PATH"

if [[ -n "$UPLOAD_TAG" && "$UPLOAD_TAG" != "$EXPECTED_TAG" ]]; then
  fail "upload tag $UPLOAD_TAG does not match synchronized release tag $EXPECTED_TAG"
fi

HEAD_SHA="$(git -C "$REPO_ROOT" rev-parse HEAD)" || fail "cannot resolve the current commit"
[[ "$HEAD_SHA" =~ ^[0-9a-f]{40}$ ]] || fail "current commit is not a full lowercase Git SHA"
RELEASE_DATABASE_ID=""

release_json() {
  gh release view "$EXPECTED_TAG" --json databaseId,isDraft,isPrerelease,tagName,targetCommitish,assets 2>/dev/null
}

validate_release() {
  local json="$1"
  local phase="$2"
  local expected_database_id="${3:-}"
  local summary target target_sha asset_names
  summary="$(node -e '
    const r=JSON.parse(process.argv[1]);
    const fields=[r.databaseId, r.isDraft, r.isPrerelease, r.tagName, r.targetCommitish];
    process.stdout.write(fields.map((v) => String(v ?? "")).join("\n"));
  ' "$json")" || fail "$phase: malformed GitHub Release metadata"
  local database_id is_draft is_prerelease tag_name
  database_id="$(sed -n '1p' <<<"$summary")"
  is_draft="$(sed -n '2p' <<<"$summary")"
  is_prerelease="$(sed -n '3p' <<<"$summary")"
  tag_name="$(sed -n '4p' <<<"$summary")"
  target="$(sed -n '5p' <<<"$summary")"
  [[ "$database_id" =~ ^[0-9]+$ ]] || fail "$phase: Release database ID is missing"
  [[ "$is_draft" == "true" ]] || fail "$phase: $EXPECTED_TAG is not a draft Release"
  [[ "$is_prerelease" == "false" ]] || fail "$phase: $EXPECTED_TAG is unexpectedly marked as a prerelease"
  [[ "$tag_name" == "$EXPECTED_TAG" ]] || fail "$phase: Release tag $tag_name does not match $EXPECTED_TAG"
  [[ -n "$target" ]] || fail "$phase: draft Release target is missing"
  target_sha="$(git -C "$REPO_ROOT" rev-parse --verify "$target^{commit}" 2>/dev/null || true)"
  [[ "$target_sha" == "$HEAD_SHA" ]] ||
    fail "$phase: draft target $target does not resolve to current commit $HEAD_SHA"
  if [[ -n "$expected_database_id" && "$database_id" != "$expected_database_id" ]]; then
    fail "$phase: draft Release identity changed during upload"
  fi

  asset_names="$(node -e '
    const r=JSON.parse(process.argv[1]);
    const names=(r.assets ?? []).map((a) => a.name).sort();
    process.stdout.write(names.join("\n"));
  ' "$json")" || fail "$phase: malformed GitHub asset metadata"
  printf '%s' "$asset_names"
}

validate_managed_inventory() {
  local asset_names="$1"
  local phase="$2"
  if [[ "$CLOBBER" == "1" ]]; then
    [[ "$asset_names" == "$EXPECTED_MANAGED" ]] ||
      fail "$phase: --clobber requires the same complete release asset pair on the matching draft"
  else
    [[ -z "$asset_names" ]] ||
      fail "$phase: draft already has release assets; use --clobber only for an exact-pair retry"
  fi
}

validate_uploaded_assets() {
  local json="$1"
  local artifact_size checksum_size checksum_sha256
  artifact_size="$(stat -f%z "$ARTIFACT_PATH")"
  checksum_size="$(stat -f%z "$CHECKSUM_PATH")"
  checksum_sha256="$(shasum -a 256 "$CHECKSUM_PATH" | awk '{print $1}')"
  node -e '
    const release=JSON.parse(process.argv[1]);
    const expected=new Map([
      [process.argv[2], {size: Number(process.argv[3]), digest: `sha256:${process.argv[4]}`}],
      [process.argv[5], {size: Number(process.argv[6]), digest: `sha256:${process.argv[7]}`}],
    ]);
    for (const asset of release.assets ?? []) {
      if (!expected.has(asset.name)) continue;
      const wanted=expected.get(asset.name);
      if (asset.state !== "uploaded" || asset.size !== wanted.size || asset.digest !== wanted.digest) process.exit(1);
      expected.delete(asset.name);
    }
    if (expected.size !== 0) process.exit(1);
  ' "$json" "$ARTIFACT_NAME" "$artifact_size" "$ARTIFACT_SHA256" \
    "$CHECKSUM_NAME" "$checksum_size" "$checksum_sha256" ||
    fail "uploaded asset state, byte size, or digest does not match the verified local pair"
}

if [[ -n "$UPLOAD_TAG" ]]; then
  BEFORE_RELEASE_JSON="$(release_json)" ||
    fail "existing draft Release $EXPECTED_TAG was not found or could not be read"
  BEFORE_ASSETS="$(validate_release "$BEFORE_RELEASE_JSON" "before build")"
  RELEASE_DATABASE_ID="$(node -e 'process.stdout.write(String(JSON.parse(process.argv[1]).databaseId ?? ""))' "$BEFORE_RELEASE_JSON")"
  EXPECTED_MANAGED="$(printf '%s\n%s' "$ARTIFACT_NAME" "$CHECKSUM_NAME" | LC_ALL=C sort)"
  validate_managed_inventory "$BEFORE_ASSETS" "before build"
fi

[[ ! -e "$ARTIFACT_PATH" && ! -e "$CHECKSUM_PATH" ]] ||
  fail "release output already exists; remove the existing exact pair before retrying"

TARGET_DIR="$(
  cd "$REPO_ROOT"
  cargo metadata --manifest-path src-tauri/Cargo.toml --no-deps --format-version 1 |
    node -e 'const fs=require("fs"); const value=JSON.parse(fs.readFileSync(0,"utf8")).target_directory; if (typeof value !== "string" || !value) process.exit(1); process.stdout.write(value)'
)" || fail "could not resolve Cargo target directory"
BUILT_DMG="$TARGET_DIR/release/bundle/dmg/$ARTIFACT_NAME"
rm -f "$BUILT_DMG"

echo "==> [1/4] Build ad-hoc-signed Apple-silicon DMG"
(
  cd "$REPO_ROOT"
  APPLE_SIGNING_IDENTITY=- cargo tauri build --bundles dmg
)
[[ -f "$BUILT_DMG" ]] || fail "expected Tauri DMG not found at $BUILT_DMG"

WORK_DIR="$(mktemp -d "$TMP_BASE/ccresdoc-macos-release.XXXXXX")"
MOUNT_POINT="$WORK_DIR/mount"
mkdir "$MOUNT_POINT"

echo "==> [2/4] Verify mounted distributable"
hdiutil verify "$BUILT_DMG" >/dev/null
DMG_ATTACHED=1
hdiutil attach -readonly -nobrowse -mountpoint "$MOUNT_POINT" "$BUILT_DMG" >/dev/null

APP_PATH="$MOUNT_POINT/CCResDoc.app"
APPLICATIONS_LINK="$MOUNT_POINT/Applications"
[[ -d "$APP_PATH" ]] || fail "mounted DMG does not contain CCResDoc.app"
[[ -L "$APPLICATIONS_LINK" ]] || fail "mounted DMG does not contain the Applications installer link"
[[ "$(readlink "$APPLICATIONS_LINK")" == "/Applications" ]] ||
  fail "mounted Applications link does not target /Applications"

INFO_PLIST="$APP_PATH/Contents/Info.plist"
plist_value() { /usr/libexec/PlistBuddy -c "Print :$1" "$INFO_PLIST" 2>/dev/null; }
[[ "$(plist_value CFBundleIdentifier)" == "$BUNDLE_ID" ]] || fail "mounted app has the wrong bundle identifier"
[[ "$(plist_value CFBundleShortVersionString)" == "$VERSION" ]] || fail "mounted app has the wrong short version"
[[ "$(plist_value CFBundleVersion)" == "$VERSION" ]] || fail "mounted app has the wrong bundle version"

APP_BINARY="$APP_PATH/Contents/MacOS/ccresdoc"
RUNTIME_ROOT="$APP_PATH/Contents/Resources/runtime-workspace/app"
DARWIN_RELATIVE_PATH="$(node -e 'const f=require(process.argv[1]).nativeCarriers["darwin-arm64"]; process.stdout.write(f.relativePath)' "$PACKAGE_FACTS")"
DARWIN_SIZE="$(node -e 'const f=require(process.argv[1]).nativeCarriers["darwin-arm64"]; process.stdout.write(String(f.sizeBytes))' "$PACKAGE_FACTS")"
DARWIN_SHA256="$(node -e 'const f=require(process.argv[1]).nativeCarriers["darwin-arm64"]; process.stdout.write(f.sha256)' "$PACKAGE_FACTS")"
ZFB_BIN="$RUNTIME_ROOT/$DARWIN_RELATIVE_PATH"

for executable in "$APP_BINARY" "$ZFB_BIN"; do
  [[ -x "$executable" ]] || fail "required executable is missing or not executable: $executable"
  [[ "$(lipo -archs "$executable")" == "arm64" ]] || fail "required executable is not arm64-only: $executable"
  codesign --verify --strict "$executable" || fail "invalid executable signature: $executable"
  SIGNATURE_DETAILS="$(codesign -d --verbose=4 "$executable" 2>&1)" ||
    fail "could not inspect executable signature: $executable"
  grep -q '^Signature=adhoc$' <<<"$SIGNATURE_DETAILS" ||
    fail "required executable does not have the intended ad-hoc signature: $executable"
done
[[ "$(stat -f%z "$ZFB_BIN")" == "$DARWIN_SIZE" ]] || fail "native runtime carrier size does not match package facts"
[[ "$(shasum -a 256 "$ZFB_BIN" | awk '{print $1}')" == "$DARWIN_SHA256" ]] ||
  fail "native runtime carrier checksum does not match package facts"

for carrier in darwin-x64 linux-arm64-gnu linux-x64-gnu win32-x64-msvc; do
  [[ ! -e "$RUNTIME_ROOT/node_modules/@takazudo/zfb-$carrier" ]] ||
    fail "non-host native runtime carrier leaked into mounted app: $carrier"
done
[[ ! -e "$RUNTIME_ROOT/node_modules/.bin/zfb" ]] || fail "Node wrapper leaked into mounted app"
NODE_LEAK="$(find "$RUNTIME_ROOT" -type f \( -name node -o -name node.exe \) -print -quit 2>/dev/null || true)"
[[ -z "$NODE_LEAK" ]] || fail "Node runtime leaked into mounted app: $NODE_LEAK"
codesign --verify --strict --deep "$APP_PATH" || fail "strict/deep verification failed for mounted app"
SIGNATURE_DETAILS="$(codesign -d --verbose=4 "$APP_PATH" 2>&1)" ||
  fail "could not inspect mounted outer app signature"
grep -q '^Signature=adhoc$' <<<"$SIGNATURE_DETAILS" ||
  fail "mounted outer app does not have the intended ad-hoc signature"

echo "==> [3/4] Run packaged WebView/runtime launch gate"
bash "$SCRIPT_DIR/test-macos-package.sh" --existing-bundle "$APP_PATH"

echo "==> [4/4] Stage exact artifact pair"
mkdir -p "$(dirname "$ARTIFACT_PATH")"
PAIR_STAGING_STARTED=1
cp "$BUILT_DMG" "$ARTIFACT_PATH"
ARTIFACT_SHA256="$(shasum -a 256 "$ARTIFACT_PATH" | awk '{print $1}')"
printf '%s  %s\n' "$ARTIFACT_SHA256" "$ARTIFACT_NAME" > "$CHECKSUM_PATH"
(
  cd "$(dirname "$ARTIFACT_PATH")"
  shasum -a 256 -c "$CHECKSUM_NAME" >/dev/null
)
PAIR_COMPLETE=1

if [[ -n "$UPLOAD_TAG" ]]; then
  PRE_UPLOAD_RELEASE_JSON="$(release_json)" || fail "draft Release disappeared before upload"
  PRE_UPLOAD_ASSETS="$(validate_release "$PRE_UPLOAD_RELEASE_JSON" "immediately before upload" "$RELEASE_DATABASE_ID")"
  validate_managed_inventory "$PRE_UPLOAD_ASSETS" "immediately before upload"
  upload_arguments=("$EXPECTED_TAG" "$ARTIFACT_PATH" "$CHECKSUM_PATH")
  if [[ "$CLOBBER" == "1" ]]; then upload_arguments+=(--clobber); fi
  gh release upload "${upload_arguments[@]}"
  AFTER_RELEASE_JSON="$(release_json)" || fail "draft Release disappeared after upload"
  AFTER_ASSETS="$(validate_release "$AFTER_RELEASE_JSON" "after upload" "$RELEASE_DATABASE_ID")"
  [[ "$AFTER_ASSETS" == "$EXPECTED_MANAGED" ]] ||
    fail "uploaded draft does not contain both and only the expected release asset names"
  validate_uploaded_assets "$AFTER_RELEASE_JSON"
fi

echo "PASS macOS release: mounted DMG, nested signatures, native runtime, and packaged WebView verified"
echo "artifact_path=$ARTIFACT_PATH"
echo "checksum_path=$CHECKSUM_PATH"
echo "sha256=$ARTIFACT_SHA256"
echo "gatekeeper=not-asserted-ad-hoc-signature"
if [[ -n "$UPLOAD_TAG" ]]; then
  echo "draft_release_id=$RELEASE_DATABASE_ID"
  echo "uploaded_tag=$EXPECTED_TAG"
else
  echo "upload=disabled"
fi
