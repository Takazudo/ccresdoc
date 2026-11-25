import assert from "node:assert/strict";
import { execFileSync, spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { refreshTokenFromWorkspaceDigest } from "./runtime-workspace-digest.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));
const identity = resolve(root, "scripts/macos-bundle-identity.mjs");
const revision = "a".repeat(40);

function sandbox(t) {
  const path = realpathSync(mkdtempSync(join(tmpdir(), "ccresdoc-acceptance-test.")));
  t.after(() => rmSync(path, { recursive: true, force: true }));
  return path;
}

function bash(code, args = []) {
  return spawnSync("bash", ["-euc", code, "acceptance-test", ...args], {
    cwd: root,
    encoding: "utf8",
  });
}

test("launch prepares a schema-v3 array-table catalog and checks an exact mounted search result", (t) => {
  const path = sandbox(t);
  const result = bash(String.raw`
source scripts/lib/launch-fixture.sh
PROBE_DIR="$1/probe with spaces"
mkdir "$PROBE_DIR"
prepare_launch_fixture
write_launch_skill "$FRESH_SLUG" "$FRESH_TERM"
jq -n --arg route "$(launch_fixture_route "$FRESH_SLUG")" --arg term "$FRESH_TERM" \
  '[{id:"smoke-source:probe-notes/skills/fresh", title:$term, body:$term, description:"", url:$route}]' > "$PROBE_DIR/index.json"
launch_fixture_in_index "$PROBE_DIR/index.json" "$FRESH_SLUG" "$FRESH_TERM"
jq '.[0].id = "claude:probe-notes/skills/fresh"' "$PROBE_DIR/index.json" > "$PROBE_DIR/wrong.json"
if launch_fixture_in_index "$PROBE_DIR/wrong.json" "$FRESH_SLUG" "$FRESH_TERM"; then exit 5; fi
printf '%s\n' "$CONFIG_PATH"
`, [path]);
  assert.equal(result.status, 0, result.stderr);
  const config = readFileSync(result.stdout.trim(), "utf8");
  assert.match(config, /^schema_version = 3$/m);
  assert.match(config, /^\[\[document_sources\]\]$/m);
  assert.match(config, /^id = "smoke-source"$/m);
  assert.match(config, /^mount = "probe-notes"$/m);
  assert.match(config, /^\[document_sources.rules\]\nkind = "claude_code"\ndefinition_version = 1$/m);
  assert.match(readFileSync(join(path, "probe with spaces/source/skills/initial/SKILL.md"), "utf8"), /name: "ccresdocinitial/);
});

test("invalid or ambiguous targets are rejected before a launch waiter is started", (t) => {
  const path = sandbox(t);
  const launch = [resolve(root, "scripts/test-launch.sh"), "--bundle", join(path, "Invalid.app")];
  const invalid = spawnSync("bash", launch, { encoding: "utf8" });
  assert.equal(invalid.status, 2);
  assert.match(invalid.stderr, /absolute path to a fresh CCResDoc.app/);
  const ambiguous = bash(String.raw`
source scripts/lib/macos-probe.sh
native_app() { printf '%s\n' 2; }
open() { touch "$1/unexpected-open"; }
PROBE_DIR="$1"
probe_launch
`, [path]);
  assert.equal(ambiguous.status, 1);
  assert.match(ambiguous.stderr, /2 running CCResDoc/);
  assert.deepEqual(existsSync(join(path, "home")), false);
  assert.deepEqual(existsSync(join(path, "open.log")), false);
});

test("non-Mac launch explicitly skips after argument validation", { skip: process.platform === "darwin" && process.arch === "arm64" }, (t) => {
  const path = join(sandbox(t), "CCResDoc.app");
  mkdirSync(path);
  const result = spawnSync("bash", ["scripts/test-launch.sh", "--bundle", path, "--controls"], {
    cwd: root,
    encoding: "utf8",
  });
  assert.equal(result.status, 77);
  assert.match(result.stderr, /SKIP launch smoke/);
  assert.doesNotMatch(result.stdout, /PASS/);
});

const lifecycle = String.raw`
source scripts/lib/macos-probe.sh
PROBE_DIR="$1"
APP_PATH=/fixture/CCResDoc.app
HOST_LOG="$PROBE_DIR/ccresdoc.log"
# Replace only native app discovery and quit. The waiter, host, process
# identity snapshots and cleanup traps below use real operating-system PIDs.
probe_host_matches() { [[ "$1" == "$mock_pid" ]]; }
native_app() {
  case "$1" in
    pids) echo "$mock_pid" ;;
    quit) [[ "$3" == "$mock_pid" ]] && kill -TERM "$3" && echo requested ;;
    *) return 1 ;;
  esac
}
sleep 60 &
mock_pid=$!
(sleep 0.1; while probe_live "$mock_pid"; do sleep 0.1; done) &
APP_OPEN_PID=$!
cleanup() {
  local status=$?
  trap - EXIT
  probe_quit || exit 1
  probe_remove_directory || exit 1
  exit "$status"
}
trap cleanup EXIT
trap 'exit 143' TERM
probe_find_host
echo "$mock_pid"
if [[ "$2" == interrupt ]]; then
  while :; do sleep 0.1; done
fi
`;

for (const mode of ["normal", "interrupt"]) {
  test(`${mode} cleanup stops the owned host and preserves foreign listener and bytes`, { timeout: 12000 }, async (t) => {
    const path = sandbox(t);
    const foreignFile = join(path, "unrelated-config.toml");
    writeFileSync(foreignFile, "unrelated fixture bytes");
    const server = createServer((_, response) => response.end("foreign"));
    server.listen(0, "127.0.0.1");
    await once(server, "listening");
    t.after(() => server.close());

    const probe = join(path, "ccresdoc-launch.fixture");
    mkdirSync(probe);
    writeFileSync(join(probe, "owned-asset"), "disposable");
    const child = spawn("bash", ["-euc", lifecycle, "acceptance-test", probe, mode], { cwd: root });
    const finished = once(child, "close");
    let errors = "";
    child.stderr.setEncoding("utf8").on("data", (chunk) => { errors += chunk; });
    const [pid] = await once(child.stdout, "data");
    const owned = Number(String(pid).trim());
    assert.ok(Number.isInteger(owned) && owned > 1);
    if (mode === "interrupt") child.kill("SIGTERM");
    const [code] = await finished;
    assert.equal(code, mode === "normal" ? 0 : 143, errors);
    assert.throws(() => process.kill(owned, 0), { code: "ESRCH" });
    assert.equal(existsSync(probe), false);
    assert.equal(readFileSync(foreignFile, "utf8"), "unrelated fixture bytes");
    const response = await fetch(`http://127.0.0.1:${server.address().port}/`);
    assert.equal(await response.text(), "foreign");
  });
}

test("retained bundle is reused for three checks and detects a failing check's bundle mutation", (t) => {
  const path = sandbox(t);
  const app = join(path, "CCResDoc.app");
  const contents = join(app, "Contents");
  const runtime = join(contents, "Resources/runtime-workspace");
  const sidecar = "app/node_modules/@takazudo/zfb-darwin-arm64/zfb";
  const digest = "b".repeat(64);
  const token = refreshTokenFromWorkspaceDigest(digest);
  mkdirSync(join(contents, "MacOS"), { recursive: true });
  mkdirSync(resolve(runtime, sidecar, ".."), { recursive: true });
  writeFileSync(join(contents, "Info.plist"), "bundle");
  writeFileSync(join(contents, "MacOS/ccresdoc"), "host");
  writeFileSync(join(runtime, sidecar), "sidecar");
  writeFileSync(join(runtime, "version.txt"), token);
  writeFileSync(join(runtime, "runtime-manifest.json"), JSON.stringify({
    refreshToken: token,
    nativeBinary: sidecar,
    workspaceDigest: { algorithm: "sha256-tree-v1", value: digest },
  }));
  const baseline = execFileSync("node", [identity, app, revision]);
  const evidence = join(path, "evidence");
  mkdirSync(evidence);
  writeFileSync(join(evidence, "bundle.identity.json"), baseline);
  const result = bash(String.raw`
source scripts/lib/macos-evidence.sh
CCRESDOC_EVIDENCE_DIR="$1"
IDENTITY_SCRIPT="$2"
APP_PATH="$3"
REVISION="$4"
check() { test "$(cat "$1/Contents/MacOS/ccresdoc")" == host; }
run_evidenced_check package check "$APP_PATH"
run_evidenced_check settings check "$APP_PATH"
run_evidenced_check launch check "$APP_PATH"
change() { echo changed > "$1/Contents/Info.plist"; return 33; }
run_evidenced_check changed change "$APP_PATH"
`, [evidence, identity, app, revision]);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /retained bundle missing or identity changed/);
  for (const name of ["package", "settings", "launch"]) {
    assert.deepEqual(readFileSync(join(evidence, `${name}.identity.json`)), baseline);
    assert.equal(readFileSync(join(evidence, `${name}.exit`), "utf8"), "0\n");
  }
  assert.equal(readFileSync(join(evidence, "changed.exit"), "utf8"), "33\n");
  assert.equal(existsSync(app), true);
});

test("sidecar ownership rejects an unrelated zfb-looking command or wrong working directory", (t) => {
  const path = sandbox(t);
  const result = bash(String.raw`
source scripts/lib/macos-probe.sh
PROBE_HOME="$1/home"
workspace="$PROBE_HOME/Library/Application Support/com.takazudo.ccresdoc/app-workspace"
carrier="$workspace/node_modules/@takazudo/zfb-darwin-arm64/zfb"
ps() {
  case "$3" in
    41) printf '41 %s dev --host 127.0.0.1 --port 4892\n' "$carrier" ;;
    42) printf '42 /unrelated/zfb dev --host 127.0.0.1 --port 4892\n' ;;
    43) printf '43 %s dev --host 127.0.0.1 --port 4892\n' "$carrier" ;;
    44) printf '43 %s dev --host 127.0.0.1 --port 4892\n' "$carrier" ;;
  esac
}
lsof() {
  if [[ "$3" == 43 ]]; then printf 'n/unrelated\n'; else printf 'n%s\n' "$workspace"; fi
}
probe_sidecar_matches 41
if probe_sidecar_matches 42 || probe_sidecar_matches 43 || probe_sidecar_matches 44; then exit 5; fi
`, [path]);
  assert.equal(result.status, 0, result.stderr);
});

test("cleanup does not signal a live group after its recorded start identity changes", async (t) => {
  const path = sandbox(t);
  const child = spawn("sleep", ["30"], { detached: true, stdio: "ignore" });
  await once(child, "spawn");
  t.after(() => {
    if (child.exitCode === null && child.signalCode === null) child.kill();
  });
  const result = bash(String.raw`
source scripts/lib/macos-probe.sh
# No wait is needed here: the deliberately foreign group remains live.
sleep() { :; }
SIDECAR_PID="$1"
SIDECAR_START="an earlier process"
probe_stop_sidecar
`, [String(child.pid), path]);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /ownership.*ambiguous/);
  assert.doesNotThrow(() => process.kill(child.pid, 0));
});
