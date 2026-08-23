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
mkdirSync(sentinelDir);
writeFileSync(sentinelLog, "");
const shellQuote = (value) => `'${value.replaceAll("'", `'"'"'`)}'`;

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

const child = spawn(resolveNativeBinary(), ["dev", "--port", "4892"], {
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

async function fetchUntil(path, includes, attempts = 120) {
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    try {
      const response = await fetch(`http://127.0.0.1:4892${path}`);
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

  const reloadResponse = await fetch("http://127.0.0.1:4892/__zfb/reload", {
    signal: reloadController.signal,
  });
  assert.equal(reloadResponse.ok, true);
  assert.ok(reloadResponse.body);
  const reloadReader = reloadResponse.body.getReader();
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
  })();
  const contentPath = join(runtimeRoot, "src", "content", "docs", "probe.mdx");
  const hmrMarker = "HMR reached the native route at 4892.";
  writeFileSync(contentPath, `${readFileSync(contentPath, "utf8")}\n\n${hmrMarker}\n`);
  await fetchUntil("/docs/probe/", hmrMarker);
  const reloadEvent = await Promise.race([
    reloadEventPromise,
    delay(10_000).then(() => { throw new Error("timed out waiting for the content reload event"); }),
  ]);
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
    processTreeSampled: process.platform !== "win32",
    processTreeChildren,
  }, null, 2));
} finally {
  reloadController.abort();
  if (process.platform === "win32") child.kill();
  else if (child.pid) {
    try { process.kill(-child.pid, "SIGTERM"); } catch {}
  }
  const exited = await Promise.race([childExit.then(() => true), delay(5_000).then(() => false)]);
  if (!exited) {
    if (process.platform === "win32") child.kill("SIGKILL");
    else if (child.pid) {
      try { process.kill(-child.pid, "SIGKILL"); } catch {}
    }
    await childExit;
  }
  await new Promise((resolve, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(4892, "127.0.0.1", () => server.close(resolve));
  });
  rmSync(stateDir, { recursive: true, force: true });
}
