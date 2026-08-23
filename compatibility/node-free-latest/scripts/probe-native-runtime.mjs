import assert from "node:assert/strict";
import { cpSync, mkdtempSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { delimiter, join } from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { setTimeout as delay } from "node:timers/promises";
import { resolveNativeBinary, probeRoot } from "./native-binary.mjs";

const stateDir = mkdtempSync(join(tmpdir(), "ccresdoc-node-free-probe-"));
const sentinelDir = join(stateDir, "sentinel-bin");
const sentinelLog = join(stateDir, "node-invocations.log");
const processLog = join(stateDir, "process-tree.log");
const runtimeRoot = join(stateDir, "workspace");
const port = 4892;
const portLock = join(tmpdir(), `ccresdoc-runtime-port-${port}.lock`);
let portLockHeld = false;
mkdirSync(sentinelDir);
writeFileSync(sentinelLog, "");
const shellQuote = (value) => `'${value.replaceAll("'", `'"'"'`)}'`;

function processIsAlive(pid) {
  if (!Number.isSafeInteger(pid) || pid <= 0) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error?.code === "EPERM";
  }
}

function acquirePortLock() {
  for (let attempt = 0; attempt < 2; attempt += 1) {
    try {
      mkdirSync(portLock);
      writeFileSync(join(portLock, "owner.json"), `${JSON.stringify({ pid: process.pid, port })}\n`);
      portLockHeld = true;
      return;
    } catch (error) {
      if (error?.code !== "EEXIST") throw error;
      let owner;
      try {
        owner = JSON.parse(readFileSync(join(portLock, "owner.json"), "utf8"));
      } catch {}
      if (!Number.isSafeInteger(owner?.pid) || owner.pid <= 0) {
        throw new Error(`runtime probe lock for fixed port ${port} is initializing or invalid`);
      }
      if (processIsAlive(owner.pid)) {
        throw new Error(`runtime probe for fixed port ${port} is already running (pid ${owner.pid})`);
      }
      rmSync(portLock, { recursive: true, force: true });
    }
  }
  throw new Error(`could not serialize runtime probe for fixed port ${port}`);
}

function releasePortLock() {
  if (!portLockHeld) return;
  rmSync(portLock, { recursive: true, force: true });
  portLockHeld = false;
}

process.once("exit", () => {
  rmSync(stateDir, { recursive: true, force: true });
  releasePortLock();
});

cpSync(probeRoot, runtimeRoot, {
  recursive: true,
  filter: (source) => !["node_modules", "dist", ".zfb", ".zfb-build"].includes(source.split(/[\\/]/).at(-1)),
});
symlinkSync(join(probeRoot, "node_modules"), join(runtimeRoot, "node_modules"), "dir");

const sentinel = process.platform === "win32" ? join(sentinelDir, "node.cmd") : join(sentinelDir, "node");
writeFileSync(sentinel, process.platform === "win32"
  ? `@echo sentinel >> "${sentinelLog}"\r\n@exit /b 97\r\n`
  : `#!/bin/sh\nprintf '%s\\n' "$*" >> ${shellQuote(sentinelLog)}\nexit 97\n`,
  { mode: 0o755 });

acquirePortLock();

const child = spawn(resolveNativeBinary(), ["dev", "--port", String(port)], {
  cwd: runtimeRoot,
  detached: process.platform !== "win32",
  env: {
    ...process.env,
    PATH: `${sentinelDir}${delimiter}${process.env.PATH ?? ""}`,
  },
  stdio: ["ignore", "pipe", "pipe"],
});
const childExit = new Promise((resolve) => {
  child.once("exit", (code, signal) => resolve({ code, signal }));
  child.once("error", (error) => resolve({ error }));
});

let serverLog = "";
let processTreeChildren = [];
const reloadController = new AbortController();
child.stdout.on("data", (chunk) => { serverLog += chunk; });
child.stderr.on("data", (chunk) => { serverLog += chunk; });

let childStopPromise;
function stopChild() {
  childStopPromise ??= (async () => {
    if (child.exitCode === null) {
      if (process.platform === "win32") child.kill();
      else if (child.pid) {
        try { process.kill(-child.pid, "SIGTERM"); } catch {}
      }
    }
    const exited = await Promise.race([childExit.then(() => true), delay(5_000).then(() => false)]);
    if (!exited) {
      if (process.platform === "win32") child.kill("SIGKILL");
      else if (child.pid) {
        try { process.kill(-child.pid, "SIGKILL"); } catch {}
      }
      await childExit;
    }
  })();
  return childStopPromise;
}

let signalShutdownStarted = false;
for (const [signal, exitCode] of [["SIGINT", 130], ["SIGTERM", 143]]) {
  process.once(signal, () => {
    if (signalShutdownStarted) return;
    signalShutdownStarted = true;
    void (async () => {
      try {
        reloadController.abort();
        await stopChild();
      } finally {
        rmSync(stateDir, { recursive: true, force: true });
        releasePortLock();
        process.exit(exitCode);
      }
    })();
  });
}

async function fetchUntil(path, includes, attempts = 120) {
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}${path}`);
      const body = await response.text();
      if (response.ok && body.includes(includes)) return body;
    } catch {}
    await delay(250);
  }
  throw new Error(`route ${path} did not contain ${JSON.stringify(includes)}\n${serverLog}`);
}

try {
  const home = await fetchUntil("/", "server-rendered");
  assert.equal(child.exitCode, null, serverLog);
  assert.match(home, /data-zfb-island="ProbeCounter"/);
  assert.match(home, /data-props="\{&quot;initial&quot;:2\}"/);
  assert.match(home, /<script type="module" src="\/assets\/islands\.js"><\/script>/);
  // The HTML cache can become ready one tick before the island asset cache on
  // a cold native start. Poll both through the same readiness helper so this
  // remains a runtime assertion instead of a scheduler-sensitive race.
  const islands = await fetchUntil("/assets/islands.js", "ProbeCounter");
  assert.match(islands, /ProbeCounter/);

  const docs = await fetchUntil("/docs/probe/", "The directive pipeline is active.");
  assert.match(docs, /data-probe-counter/);
  assert.match(docs, /href="https:\/\/example\.com"/);

  const reloadResponse = await fetch(`http://127.0.0.1:${port}/__zfb/reload`, {
    signal: reloadController.signal,
  });
  assert.equal(reloadResponse.ok, true);
  assert.ok(reloadResponse.body);
  const reloadReader = reloadResponse.body.getReader();
  // Attach both outcomes immediately so cleanup-triggered aborts cannot become
  // unhandled rejections when an earlier runtime assertion fails.
  const reloadEventPromise = (async () => {
    const decoder = new TextDecoder();
    let transcript = "";
    while (true) {
      const { done, value } = await reloadReader.read();
      if (done) throw new Error(`reload stream ended before an event: ${transcript}`);
      transcript += decoder.decode(value, { stream: true });
      const match = transcript.match(/event:\s*(page|islands)\b/);
      if (match) return match[1];
    }
  })().then(
    (event) => ({ event }),
    (error) => ({ error }),
  );
  const contentPath = join(runtimeRoot, "src", "content", "docs", "probe.mdx");
  const hmrMarker = "HMR reached the native route at 4892.";
  writeFileSync(contentPath, `${readFileSync(contentPath, "utf8")}\n\n${hmrMarker}\n`);
  await fetchUntil("/docs/probe/", hmrMarker);
  const reloadResult = await Promise.race([
    reloadEventPromise,
    delay(10_000).then(() => { throw new Error("timed out waiting for the content reload event"); }),
  ]);
  if (reloadResult.error) throw reloadResult.error;
  const reloadEvent = reloadResult.event;
  reloadController.abort();
  assert.equal(child.exitCode, null, serverLog);

  if (process.platform !== "win32") {
    const tree = spawnSync("ps", ["-eo", "pid=,ppid=,args="], { encoding: "utf8" });
    writeFileSync(processLog, tree.stdout);
    const descendants = tree.stdout.split("\n").filter((line) => line.includes(String(child.pid)) || line.includes(runtimeRoot));
    assert.ok(descendants.some((line) => line.trimStart().startsWith(`${child.pid} `)), descendants.join("\n"));
    assert.equal(descendants.some((line) => /plugin-host\.mjs/.test(line)), false, descendants.join("\n"));
    processTreeChildren = descendants.map((line) => line
      .replace(new RegExp(`${probeRoot.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}/node_modules/@takazudo/zfb-[^/\\s]+/zfb(?:\\.exe)?`, "g"), "<native-zfb>")
      .replaceAll(runtimeRoot, "<isolated-workspace>")
      .replace(/\s+/g, " ")
      .trim()
      .replace(/^\d+ \d+ /, "<pid> <ppid> "));
  }

  await delay(750);
  assert.equal(readFileSync(sentinelLog, "utf8"), "");
  console.log(JSON.stringify({
    status: "passed",
    homeRoute: "/",
    docsRoute: "/docs/probe/",
    islandMarker: "ProbeCounter",
    contentHmrMarker: hmrMarker,
    contentReloadEvent: reloadEvent,
    nodeSentinelInvocations: 0,
    fixedPortSerialized: true,
    portReleasedAfterTeardown: true,
    processTreeSampled: process.platform !== "win32",
    processTreeChildren,
  }, null, 2));
} finally {
  reloadController.abort();
  await stopChild();
  try {
    await new Promise((resolve, reject) => {
      const server = createServer();
      server.once("error", reject);
      server.listen(port, "127.0.0.1", () => server.close(resolve));
    });
  } finally {
    rmSync(stateDir, { recursive: true, force: true });
    releasePortLock();
  }
}
