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
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { delimiter, dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import {
  assertAllowlistedInventory,
  assertRuntimeRenderedPrivacy,
  assertRuntimeWorkspacePrivacy,
} from "./runtime-workspace-files.mjs";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const stagedRoot = join(repoRoot, "src-tauri", "runtime-workspace");
const manifest = JSON.parse(readFileSync(join(stagedRoot, "runtime-manifest.json"), "utf8"));
const sourceThemeCatalog = JSON.parse(
  readFileSync(join(repoRoot, "app", "public", "theme-packs", "index.json"), "utf8"),
);
const probeRoot = mkdtempSync(join(tmpdir(), "ccresdoc-tauri-runtime-"));
const workspace = join(probeRoot, "app-workspace");
const sentinelDir = join(probeRoot, "sentinel-bin");
const sentinelLog = join(probeRoot, "node-invocations.log");
const processSamples = [];
let renderedPrivacyResponses = 0;
let processSampleFailure;
const port = 4892;
const portLock = join(tmpdir(), `ccresdoc-runtime-port-${port}.lock`);
let portLockHeld = false;
const fontUrlPattern = /url\(\s*(["']?)([^"')]+)\1\s*\)/g;

// Covers setup failures that happen before the launch lifecycle enters its
// async try/finally. Normal cleanup removes these paths first; force makes the
// exit hook idempotent.
process.once("exit", () => {
  rmSync(probeRoot, { recursive: true, force: true });
  releasePortLock();
});

cpSync(join(stagedRoot, "app"), workspace, { recursive: true, dereference: true });
assertAllowlistedInventory(workspace);
const privacyAudit = assertRuntimeWorkspacePrivacy(workspace, {
  // The temporary probe path is a synthetic configured-root candidate. It is
  // passed only as a rejection input and is never written to the workspace.
  forbiddenPaths: [probeRoot],
});
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
      if (processIsAlive(owner?.pid)) {
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

async function assertPortReleased(context) {
  await new Promise((resolveCheck, rejectCheck) => {
    const server = createServer();
    server.once("error", (error) => {
      rejectCheck(new Error(`fixed port ${port} is unavailable ${context}: ${error.message}`));
    });
    server.listen({ host: "127.0.0.1", port, exclusive: true }, () => {
      server.close((error) => {
        if (error) rejectCheck(error);
        else resolveCheck();
      });
    });
  });
}

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

async function fetchOk(path) {
  const response = await fetch(`http://127.0.0.1:${port}${path}`);
  const body = await response.text();
  assert.equal(response.ok, true, `${path} returned HTTP ${response.status}`);
  return body;
}

async function assertThemeAssetParity() {
  // The staged manifest and served index must describe the same current
  // catalog. Do not use a pack count here: adding/removing a published pack
  // must exercise this probe without a test edit.
  assert.equal(manifest.themeAssets.packs, sourceThemeCatalog.packs.length);
  assert.deepEqual(JSON.parse(await fetchOk("/theme-packs/index.json")), sourceThemeCatalog);
  let servedFiles = 1; // index.json

  for (const pack of sourceThemeCatalog.packs) {
    if (pack.slug === "default") continue;
    const css = await fetchOk(`/theme-packs/${pack.slug}/pack.css?v=${encodeURIComponent(pack.meta.version)}`);
    servedFiles += 1;
    const referencedFonts = new Set();
    for (const match of css.matchAll(fontUrlPattern)) {
      const url = match[2].trim();
      if (url.startsWith("data:") || url.startsWith("#")) continue;
      assert.match(url, /^\.\/fonts\/[A-Za-z0-9._-]+$/, `${pack.slug} has an unsafe font URL`);
      referencedFonts.add(url.slice(2));
    }
    for (const font of referencedFonts) {
      await fetchOk(`/theme-packs/${pack.slug}/${font}`);
      servedFiles += 1;
    }
    // Every generated font-bearing pack must expose the families its catalog
    // metadata promises. The CSS check catches a stale index/CSS pair even if
    // all the files happen to be present.
    for (const family of pack.meta.fonts.loaded) {
      const escaped = family.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      assert.match(css, new RegExp(`font-family:\\s*["']${escaped}["']`));
    }
  }
  assert.equal(manifest.themeAssets.files, servedFiles, "served theme asset count must match the staged manifest");
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
  await assertPortReleased("after process-group teardown");
}

let signalShutdownStarted = false;
for (const [signal, exitCode] of [["SIGINT", 130], ["SIGTERM", 143]]) {
  process.once(signal, () => {
    if (signalShutdownStarted) return;
    signalShutdownStarted = true;
    void (async () => {
      try {
        if (activeChild) await stop(activeChild);
      } finally {
        rmSync(probeRoot, { recursive: true, force: true });
        releasePortLock();
        process.exit(exitCode);
      }
    })();
  });
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

function assertRenderedPrivacy(path, body) {
  assertRuntimeRenderedPrivacy(path, body, { forbiddenPaths: [probeRoot] });
  renderedPrivacyResponses += 1;
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
    const home = await fetchUntil("/", "CCResDoc Resources");
    assertRenderedPrivacy("/", home);
    assert.match(home, /data-zfb-island=/, "representative SSR page must include hydrated islands");
    assert.doesNotMatch(home, /data-home-page/, "the root alias must not render a marketing home");
    assert.match(home, /<a[^>]*(?:href=(?:\/docs\/|\"\/docs\/\")[^>]*data-header-logo|data-header-logo(?:=true|=\"true\")[^>]*href=(?:\/docs\/|\"\/docs\/\"))/, "root alias must link its logo to /docs/");
    const assets = home.match(/\/assets\/[A-Za-z0-9._-]+/g) ?? [];
    assert.ok(assets.length > 0, "representative SSR route must reference packaged assets");

    const docs = await fetchUntil("/docs/", "CCResDoc Resources");
    assertRenderedPrivacy("/docs/", docs);
    for (const [signal, pattern] of [
      ["CCResDoc Resources", /CCResDoc Resources/],
      ["data-header-logo", /data-header-logo/],
      ["data-theme-pack", /data-theme-pack/],
      ["ThemePackSwitcher", /data-zfb-island(?:=ThemePackSwitcher|=\"ThemePackSwitcher\")/],
    ]) {
      assert.match(home, pattern, `root and /docs/ must share ${signal}`);
      assert.match(docs, pattern, `root and /docs/ must share ${signal}`);
    }
    assert.match(docs, /data-zfb-island=/, "docs route must preserve hydration markers");
    assert.match(docs, /Choose a resource category below\./, "first accepted docs response must contain the populated landing content");
    assert.match(docs, /<a[^>]*(?:href=(?:\/docs\/|\"\/docs\/\")[^>]*data-header-logo|data-header-logo(?:=true|=\"true\")[^>]*href=(?:\/docs\/|\"\/docs\/\"))/, "docs logo must link to /docs/");
    assert.match(docs, /data-header-nav(?:=true|=\"true\")/, "docs shell must expose its header nav seam");
    assert.match(docs, /Claude/, "header nav must keep the permanent Claude category");
    assert.match(docs, /Codex/, "header nav must keep the permanent Codex category");
    const headerNav = docs.match(/<nav[^>]*data-header-nav[^>]*>([\s\S]*?)<\/nav>/);
    assert.ok(headerNav, "docs shell must include a header nav element");
    assert.match(headerNav[1], /Claude/);
    assert.match(headerNav[1], /Codex/);
    assert.match(docs, /data-theme-pack(?:=default|=\"default\")/, "docs shell must bootstrap the default theme pack");
    assert.match(docs, /data-zfb-island(?:=ThemePackSwitcher|=\"ThemePackSwitcher\")/, "docs shell must hydrate the theme-pack switcher");
    assert.match(docs, /data-zd-theme-pack-loading|zudo-doc-theme-pack/, "docs shell must include no-flash theme bootstrap");
    await assertThemeAssetParity();
    const missing = await fetch(`http://127.0.0.1:${port}/definitely-missing/`);
    assert.equal(missing.status, 404, "missing routes must return the host-owned 404 response");
    assertRenderedPrivacy("/definitely-missing/", await missing.text());

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
      const hmrBody = await fetchUntil("/docs/", marker);
      assertRenderedPrivacy("/docs/ (HMR)", hmrBody);
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
  acquirePortLock();
  await assertPortReleased("before first launch");
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
    renderedPrivacyResponses,
    privacyAudit,
    processGroupShutdown: true,
    fixedPortSerialized: true,
    portReleasedAfterEachLaunch: true,
    host: manifest.host,
    macosArm64AppWebViewHostGate: "not-run-by-runtime-probe",
  }, null, 2));
} finally {
  try {
    if (activeChild) await stop(activeChild);
  } finally {
    rmSync(probeRoot, { recursive: true, force: true });
    releasePortLock();
  }
}
