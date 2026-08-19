#!/usr/bin/env node

import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import {
  cpSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const stagedRoot = join(repoRoot, "src-tauri", "runtime-workspace");
const manifest = JSON.parse(readFileSync(join(stagedRoot, "runtime-manifest.json"), "utf8"));
const probeRoot = mkdtempSync(join(tmpdir(), "ccresdoc-tauri-runtime-"));
const workspace = join(probeRoot, "app-workspace");
const sentinelDir = join(probeRoot, "sentinel-bin");
const sentinelLog = join(probeRoot, "node-invocations.log");
const processSamples = [];
let processSampleFailure;
const port = 4892;

cpSync(join(stagedRoot, "app"), workspace, { recursive: true, dereference: true });
mkdirSync(sentinelDir);
writeFileSync(sentinelLog, "");
const sentinel = join(sentinelDir, process.platform === "win32" ? "node.cmd" : "node");
if (process.platform === "win32") {
  writeFileSync(sentinel, `@echo %*>>"${sentinelLog}"\r\n@exit /b 97\r\n`);
} else {
  const quotedLog = `'${sentinelLog.replaceAll("'", `'"'"'`)}'`;
  writeFileSync(sentinel, `#!/bin/sh\nprintf '%s\\n' "$*" >> ${quotedLog}\nexit 97\n`, { mode: 0o755 });
}

const binary = join(workspace, "node_modules", ...manifest.hostPackage.split("/"), process.platform === "win32" ? "zfb.exe" : "zfb");
let activeChild;

function sampleProcesses(child) {
  if (process.platform === "win32") return [];
  const result = spawnSync("ps", ["-eo", "pid=,ppid=,pgid=,args="], { encoding: "utf8" });
  const rows = result.stdout.split("\n").filter((line) => line.includes(workspace) || line.match(new RegExp(`^\\s*${child.pid}\\s`)));
  processSamples.push(...rows.map((row) => row.replaceAll(workspace, "<workspace>").replace(/\s+/g, " ").trim()));
  assert.equal(rows.some((row) => /plugin-host\.mjs|\/node(?:\s|$)/.test(row)), false, rows.join("\n"));
  return rows;
}

async function fetchUntil(path, marker, attempts = 240) {
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    if (processSampleFailure) throw processSampleFailure;
    try {
      const response = await fetch(`http://127.0.0.1:${port}${path}`);
      const body = await response.text();
      if (response.ok && body.includes(marker)) return body;
    } catch {}
    if (activeChild?.exitCode !== null) throw new Error(`zfb exited before ${path} became ready\n${activeChild.output}`);
    await delay(250);
  }
  throw new Error(`timed out waiting for ${path} to contain ${JSON.stringify(marker)}`);
}

async function stop(child) {
  if (child.exitCode === null) {
    if (process.platform === "win32") child.kill();
    else {
      try { process.kill(-child.pid, "SIGTERM"); } catch {}
    }
  }
  const exited = await Promise.race([child.exited.then(() => true), delay(5_000).then(() => false)]);
  if (!exited) {
    if (process.platform === "win32") child.kill("SIGKILL");
    else {
      try { process.kill(-child.pid, "SIGKILL"); } catch {}
    }
    await child.exited;
  }
  await delay(250);
  if (process.platform !== "win32") {
    const tree = spawnSync("ps", ["-eo", "pid=,ppid=,pgid=,args="], { encoding: "utf8" }).stdout;
    assert.equal(tree.includes(workspace), false, `workspace process survived shutdown:\n${tree}`);
  }
}

function launch() {
  const child = spawn(binary, ["dev", "--port", String(port)], {
    cwd: workspace,
    detached: process.platform !== "win32",
    env: { ...process.env, PATH: `${sentinelDir}${delimiter}${process.env.PATH ?? ""}` },
    stdio: ["ignore", "pipe", "pipe"],
  });
  child.output = "";
  child.stdout.on("data", (chunk) => { child.output += chunk; });
  child.stderr.on("data", (chunk) => { child.output += chunk; });
  child.exited = new Promise((resolveExit) => child.once("exit", resolveExit));
  return child;
}

async function runOnce({ hmr }) {
  const child = launch();
  activeChild = child;
  const reloadController = new AbortController();
  const sampler = setInterval(() => {
    try {
      sampleProcesses(child);
    } catch (error) {
      processSampleFailure ??= error;
    }
  }, 100);
  try {
    const home = await fetchUntil("/", "CCResDoc");
    assert.match(home, /data-zfb-island=/, "representative SSR page must include hydrated islands");
    const assets = home.match(/\/assets\/[A-Za-z0-9._-]+/g) ?? [];
    assert.ok(assets.length > 0, "representative SSR route must reference packaged assets");

    const docs = await fetchUntil("/docs/", "Claude Code Resources");
    assert.match(docs, /data-zfb-island=/, "docs route must preserve hydration markers");
    const missing = await fetch(`http://127.0.0.1:${port}/definitely-missing/`);
    assert.equal(missing.status, 404, "missing routes must return the host-owned 404 response");
    await missing.text();

    if (hmr) {
      const reload = await fetch(`http://127.0.0.1:${port}/__zfb/reload`, {
        signal: reloadController.signal,
      });
      assert.equal(reload.ok, true);
      const reader = reload.body.getReader();
      // Attach the rejection handler synchronously. Aborting the stream during
      // cleanup must never become an unhandled rejection under Node's strict
      // promise policy.
      const event = (async () => {
        let transcript = "";
        const decoder = new TextDecoder();
        while (true) {
          const { done, value } = await reader.read();
          if (done) throw new Error(`reload stream ended: ${transcript}`);
          transcript += decoder.decode(value, { stream: true });
          if (/event:\s*(page|islands)\b/.test(transcript)) return transcript;
        }
      })().then(
        (transcript) => ({ transcript }),
        (error) => ({ error }),
      );
      // Content edits exercise the generator -> zfb content-watch path. Page
      // source edits restart the dev server and close the SSE stream, which is
      // a different lifecycle and made this probe race Node's unhandled-
      // rejection policy before it could report the actual runtime state.
      const content = join(workspace, "src", "content", "docs", "index.mdx");
      const marker = `Packaged HMR probe ${Date.now()}`;
      const original = readFileSync(content, "utf8");
      assert.match(original, /Choose a resource category below\./);
      writeFileSync(content, `${original}\n\n${marker}\n`);
      await fetchUntil("/docs/", marker);
      const reloadResult = await Promise.race([
        event,
        delay(10_000).then(() => { throw new Error("reload event timeout"); }),
      ]);
      if (reloadResult.error) throw reloadResult.error;
      await reader.cancel();
    }

    sampleProcesses(child);
    if (processSampleFailure) throw processSampleFailure;
    assert.equal(readFileSync(sentinelLog, "utf8"), "", "Node sentinel was invoked");
    assert.equal(child.exitCode, null, child.output);
  } finally {
    reloadController.abort();
    clearInterval(sampler);
    await stop(child);
    activeChild = undefined;
  }
}

try {
  await runOnce({ hmr: true });
  // A second full start proves the retry/relaunch path can reclaim :4892 after
  // process-group teardown instead of inheriting a stale listener.
  await runOnce({ hmr: false });
  assert.equal(readFileSync(sentinelLog, "utf8"), "");
  console.log(JSON.stringify({
    status: "passed",
    port,
    launches: 2,
    routes: ["/", "/docs/", "/definitely-missing/"],
    hmr: true,
    refreshToken: manifest.refreshToken,
    resolvedPluginDescriptors: 0,
    nodeSentinelInvocations: 0,
    processSamples: processSamples.length,
    processGroupShutdown: true,
    host: manifest.host,
    macosArm64PackageContract: process.platform === "darwin" && process.arch === "arm64" ? "tested" : "not-run-on-this-host",
  }, null, 2));
} finally {
  if (activeChild) await stop(activeChild);
  rmSync(probeRoot, { recursive: true, force: true });
}
