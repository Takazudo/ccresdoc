#!/usr/bin/env node

/*
 * Wave 2 integration confirmation for #186.
 *
 * This runner deliberately owns the browser-facing assertions that the Rust
 * readiness probe cannot prove by itself.  It starts the same native zfb
 * binary as probe-tauri-runtime.mjs in a temporary copy of the staged app,
 * adds a synthetic 361-page tree, and uses WebKit's DOMParser as an oracle.
 * The Rust HTML tokenizer is never imported here.
 *
 * Prerequisite (from the app package, where Playwright is pinned):
 *   pnpm --dir app install --frozen-lockfile
 *   pnpm --dir app exec playwright install webkit
 *
 * Run from the repository root:
 *   node scripts/confirm-hydration-readiness.mjs
 */

import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { spawn, spawnSync } from "node:child_process";
import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { delimiter, dirname, join, relative, resolve, sep } from "node:path";
import { tmpdir } from "node:os";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import {
  assertAllowlistedInventory,
  assertRuntimeRenderedPrivacy,
  assertRuntimeWorkspacePrivacy,
} from "./runtime-workspace-files.mjs";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const appRoot = join(repoRoot, "app");
const stagedRoot = join(repoRoot, "src-tauri", "runtime-workspace");
const stagedApp = join(stagedRoot, "app");
const manifestPath = join(stagedRoot, "runtime-manifest.json");
const port = 4892;
const syntheticPageCount = 361;
const readyTimeoutMs = 300_000;
const pollIntervalMs = 50;
const probeIoTimeoutMs = 2_000;
const liveWorkspace = process.env.HOME
  ? join(process.env.HOME, "Library", "Application Support", "com.takazudo.ccresdoc", "app-workspace")
  : null;

const appRequire = createRequire(join(appRoot, "package.json"));

function usage() {
  console.log(`Usage: node scripts/confirm-hydration-readiness.mjs

Requires the app package's frozen install and a WebKit browser:
  pnpm --dir app install --frozen-lockfile
  pnpm --dir app exec playwright install webkit`);
}

if (process.argv.includes("--help") || process.argv.includes("-h")) {
  usage();
  process.exit(0);
}

function ensureStagedRuntime() {
  // The stage is ignored build output. Always regenerate it so a standalone
  // invocation cannot accidentally probe CSS/bridge bytes left by an older
  // checkout in this worktree.
  const result = spawnSync(process.execPath, [join(repoRoot, "scripts", "stage-runtime-workspace.mjs")], {
    cwd: repoRoot,
    encoding: "utf8",
    stdio: "inherit",
  });
  assert.equal(result.status, 0, "staging the temporary runtime workspace failed");
}

function assertApplyReadinessContract() {
  const mainSource = readFileSync(join(repoRoot, "src-tauri", "src", "main.rs"), "utf8");
  const commandsSource = readFileSync(join(repoRoot, "src-tauri", "src", "settings_commands.rs"), "utf8");
  const runtimeSource = readFileSync(join(repoRoot, "src-tauri", "src", "runtime.rs"), "utf8");
  const settingsSource = readFileSync(join(repoRoot, "app", "src", "config", "settings.ts"), "utf8");
  assert.match(
    mainSource,
    /fn relaunch_previous_runtime[\s\S]*?probe_resource_readiness\(/,
    "ApplyCoordinator relaunch must use the widened module-aware readiness probe",
  );
  assert.match(
    mainSource,
    /fn launch\([\s\S]*?probe_resource_readiness\(/,
    "main.rs::launch must use the widened module-aware readiness probe",
  );
  assert.match(
    mainSource,
    /fn start_launch\([\s\S]*?with_serialized_apply\(\|\| launch\(/,
    "retry/start_launch must route through the serialized ApplyCoordinator path",
  );
  assert.match(
    commandsSource,
    /fn retry_launch[\s\S]*?crate::start_launch\(/,
    "retry_launch must route through start_launch",
  );
  assert.match(
    commandsSource,
    /fn apply_saved[\s\S]*?launch\(app, generation, effective\)/,
    "settings apply must route restart transitions through main.rs::launch",
  );
  assert.match(
    runtimeSource,
    /fn apply_coordinator_launch_boundary_publishes_ready_only_after_module_probe[\s\S]*?probe_resource_readiness\(/,
    "the executable coordinator/readiness integration sample must remain present",
  );
  assert.match(settingsSource, /dynamicPageTransition:\s*true/, "the router-swap confirmation must run with dynamicPageTransition enabled");
}

function assertTemporaryWorkspace(path) {
  const normalized = resolve(path);
  const temporaryRoot = resolve(tmpdir());
  assert(
    normalized === temporaryRoot || normalized.startsWith(`${temporaryRoot}${sep}`),
    `probe workspace must remain under the OS temporary directory: ${normalized}`,
  );
  if (liveWorkspace) {
    assert.notEqual(normalized, resolve(liveWorkspace), "the live app workspace is never a probe target");
    assert(!normalized.startsWith(`${resolve(liveWorkspace)}${sep}`), "probe path is nested below the live app workspace");
  }
}

function makeSyntheticFixture(workspace) {
  const docsRoot = join(workspace, "src", "content", "docs");
  const fixtureRoot = join(docsRoot, "hydration-pages");
  mkdirSync(fixtureRoot, { recursive: true });

  for (let index = 0; index < syntheticPageCount; index += 1) {
    const slug = `page-${String(index).padStart(3, "0")}`;
    const next = `page-${String((index + 1) % syntheticPageCount).padStart(3, "0")}`;
    const previous = `page-${String((index + syntheticPageCount - 1) % syntheticPageCount).padStart(3, "0")}`;
    writeFileSync(
      join(fixtureRoot, `${slug}.mdx`),
      `---\ntitle: Hydration sample ${String(index).padStart(3, "0")}\ndescription: Synthetic cold-build page for hydration readiness.\nsidebar_position: ${index + 10}\n---\n\n# Hydration sample ${String(index).padStart(3, "0")}\n\nThis page is synthetic and exists only for the cold-build confirmation.\n\n[Previous sample](/docs/hydration-pages/${previous}/) · [Next sample](/docs/hydration-pages/${next}/)\n`,
    );
  }

  // The first navigation is user-like too: the root document supplies a
  // normal client-router link into the synthetic tree instead of a direct
  // location assignment.
  const landing = join(docsRoot, "index.mdx");
  const original = readFileSync(landing, "utf8");
  writeFileSync(
    landing,
    `${original.trimEnd()}\n\n[Open hydration sample 001](/docs/hydration-pages/page-001/)\n`,
  );

  const files = readdirSync(fixtureRoot).filter((name) => name.endsWith(".mdx"));
  assert.equal(files.length, syntheticPageCount, "the cold-build fixture must contain exactly 361 pages");
  return { docsRoot, fixtureRoot, files };
}

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
  const lock = join(tmpdir(), `ccresdoc-runtime-port-${port}.lock`);
  for (let attempt = 0; attempt < 2; attempt += 1) {
    try {
      mkdirSync(lock);
      writeFileSync(join(lock, "owner.json"), `${JSON.stringify({ pid: process.pid, port })}\n`);
      return lock;
    } catch (error) {
      if (error?.code !== "EEXIST") throw error;
      let owner;
      try {
        owner = JSON.parse(readFileSync(join(lock, "owner.json"), "utf8"));
      } catch {}
      if (!Number.isSafeInteger(owner?.pid) || owner.pid <= 0 || processIsAlive(owner.pid)) {
        throw new Error(`runtime probe for fixed port ${port} is already running`);
      }
      rmSync(lock, { recursive: true, force: true });
    }
  }
  throw new Error(`could not serialize runtime probe for fixed port ${port}`);
}

async function assertPortAvailable() {
  const { createServer } = await import("node:net");
  await new Promise((resolveCheck, rejectCheck) => {
    const server = createServer();
    server.once("error", (error) => rejectCheck(new Error(`fixed port ${port} is unavailable: ${error.message}`)));
    server.listen({ host: "127.0.0.1", port, exclusive: true }, () => {
      server.close((error) => (error ? rejectCheck(error) : resolveCheck()));
    });
  });
}

function spawnZfb(workspace, sentinelDir, sentinelLog, manifest) {
  const binary = join(
    workspace,
    "node_modules",
    ...manifest.hostPackage.split("/"),
    process.platform === "win32" ? "zfb.exe" : "zfb",
  );
  assert(existsSync(binary), `native zfb binary is missing from the staged workspace: ${binary}`);
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

function assertNoUnexpectedWorkspaceProcesses(workspace) {
  if (process.platform === "win32") return;
  const result = spawnSync("ps", ["-eo", "pid=,ppid=,pgid=,args="], { encoding: "utf8" });
  const rows = result.stdout.split("\n").filter((line) => line.includes(workspace));
  assert.equal(
    rows.some((row) => /plugin-host\.mjs|\/node(?:\s|$)/.test(row)),
    false,
    `unexpected Node/plugin-host process in the temporary workspace:\n${rows.join("\n")}`,
  );
}

async function stopZfb(child, workspace) {
  if (child.exitCode === null) {
    if (process.platform === "win32") child.kill();
    else {
      try { process.kill(-child.pid, "SIGTERM"); } catch {}
    }
  }
  const exited = await Promise.race([
    child.exited.then(() => true),
    delay(5_000).then(() => false),
  ]);
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
    assert(!tree.includes(workspace), `workspace process survived shutdown:\n${tree}`);
  }
  await assertPortAvailable();
}

async function fetchText(path) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), probeIoTimeoutMs);
  try {
    const response = await fetch(`http://127.0.0.1:${port}${path}`, {
      signal: controller.signal,
      headers: { connection: "close" },
    });
    return { status: response.status, body: await response.text() };
  } catch {
    return { status: 0, body: "" };
  } finally {
    clearTimeout(timeout);
  }
}

async function domOracle(oraclePage, html) {
  return oraclePage.evaluate((source) => {
    const document = new DOMParser().parseFromString(source, "text/html");
    const base = new URL("http://127.0.0.1:4892/docs/");
    const modules = [];
    for (const script of document.querySelectorAll("script[src]")) {
      const type = script.getAttribute("type")?.trim().toLowerCase();
      if (type !== "module") continue;
      const raw = script.getAttribute("src")?.trim();
      if (!raw || raw.includes("\r") || raw.includes("\n")) continue;
      let url;
      try { url = new URL(raw, base); } catch { continue; }
      if (url.origin !== base.origin || url.username || url.password) continue;
      modules.push(`${url.pathname}${url.search}`);
    }
    return {
      hasShellMarker: document.body?.textContent?.includes("CCResDoc") ?? false,
      hasIslandMarkers: document.querySelector("[data-zfb-island], [data-zfb-island-skip-ssr]") !== null,
      modulePaths: [...new Set(modules)],
    };
  }, html);
}

async function inspectReadiness(oraclePage) {
  const shell = await fetchText("/docs/");
  const claude = await fetchText("/docs/claude/");
  const codex = await fetchText("/docs/codex/");
  const dom = shell.status === 200 ? await domOracle(oraclePage, shell.body) : {
    hasShellMarker: false,
    hasIslandMarkers: false,
    modulePaths: [],
  };
  const moduleStatuses = [];
  for (const path of dom.modulePaths) {
    const response = await fetchText(path);
    moduleStatuses.push({ path, status: response.status });
  }
  // The cold-build sidecar is intentionally exercised without Rust resource
  // generation. The host's first readiness input is the neutral /docs shell;
  // the independent oracle then checks its island entry before the browser is
  // allowed to navigate. Overview responses are retained as diagnostics and
  // are covered by the Rust readiness unit/integration gates.
  const hostReady = shell.status === 200
    && dom.hasShellMarker
    && dom.modulePaths.length > 0;
  const failedModules = moduleStatuses.filter(({ status }) => status < 200 || status >= 300);
  // This is the independent version of the #184 invariant: a shell that the
  // host could classify as Ready must never expose a missing module entry.
  assert.equal(
    hostReady && failedModules.length > 0,
    false,
    `host readiness preceded module readiness: ${JSON.stringify({ hostReady, failedModules, modulePaths: dom.modulePaths })}`,
  );
  return {
    at: Date.now(),
    hostReady,
    modulesReady: failedModules.length === 0 && dom.modulePaths.length > 0,
    ready: hostReady && failedModules.length === 0,
    shellStatus: shell.status,
    overviewStatuses: { claude: claude.status, codex: codex.status },
    moduleStatuses,
  };
}

async function waitForHostReady(oraclePage, child, samples, workspace) {
  const deadline = Date.now() + readyTimeoutMs;
  let nextProcessSample = 0;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) throw new Error(`zfb exited before readiness:\n${child.output}`);
    if (Date.now() >= nextProcessSample) {
      assertNoUnexpectedWorkspaceProcesses(workspace);
      nextProcessSample = Date.now() + 250;
    }
    const sample = await inspectReadiness(oraclePage);
    samples.push(sample);
    if (sample.ready) return sample;
    await delay(pollIntervalMs);
  }
  throw new Error(`timed out waiting for independent host readiness after ${readyTimeoutMs}ms`);
}

async function assertBrowserSurface(browser, firstReady, samples, workspace) {
  const context = await browser.newContext();
  const page = await context.newPage();
  const mainDocumentRequests = [];
  const moduleRequests = new Map();
  const failedRequests = [];
  page.on("request", (request) => {
    if (request.isNavigationRequest() && request.frame() === page.mainFrame()) {
      mainDocumentRequests.push(request.url());
    }
    if (request.resourceType() === "script" || request.url().includes("islands-chunk-")) {
      moduleRequests.set(request.url(), { url: request.url(), status: null });
    }
  });
  page.on("response", (response) => {
    const record = moduleRequests.get(response.url());
    if (record) record.status = response.status();
  });
  page.on("requestfailed", (request) => {
    if (request.resourceType() === "script" || request.url().includes("islands-chunk-")) {
      failedRequests.push({ url: request.url(), error: request.failure()?.errorText ?? "unknown" });
    }
  });

  const docsUrl = `http://127.0.0.1:${port}/docs/`;
  await page.goto(docsUrl, { waitUntil: "domcontentloaded", timeout: 30_000 });
  await page.waitForFunction(
    () => document.documentElement.hasAttribute("data-ccresdoc-load-controls-ready"),
    undefined,
    { timeout: 30_000 },
  );
  assertNoUnexpectedWorkspaceProcesses(workspace);
  assert.equal(mainDocumentRequests.length, 1, `cold start must have exactly one main-frame document navigation: ${mainDocumentRequests}`);
  assert.equal(failedRequests.length, 0, `WebKit reported failed module requests: ${JSON.stringify(failedRequests)}`);
  const moduleFailures = [...moduleRequests.values()].filter((record) => record.status === null || record.status < 200 || record.status >= 300);
  assert.equal(moduleFailures.length, 0, `module entry/chunk response was not 2xx: ${JSON.stringify(moduleFailures)}`);
  assert(moduleRequests.size > 0, "WebKit did not request an island module entry");

  const initialTheme = await page.locator("html").getAttribute("data-theme");
  const themeToggle = page.locator('[data-zfb-island="ThemeToggle"] button').first();
  await themeToggle.click();
  await page.waitForFunction(
    (before) => document.documentElement.getAttribute("data-theme") !== before,
    initialTheme,
    { timeout: 5_000 },
  );

  const launcher = page.locator('[data-zfb-island="ThemePackSwitcher"] [data-switcher-launcher]').first();
  await launcher.click();
  await page.locator('[data-zfb-island="ThemePackSwitcher"] [data-switcher-card][role="dialog"]').waitFor({ state: "visible", timeout: 5_000 });
  await launcher.click();

  // Capture the pending marker at frame cadence during a client-router swap.
  // The allowed absence is bounded to the remount's synchronous load pass;
  // long absence would be a visible flash and would make controls inert.
  await page.evaluate(() => {
    const state = { missingSince: null, missingMs: 0, missingFrames: 0, samples: 0, stopped: false };
    (window).__ccresdocHydrationSamples = state;
    const sample = () => {
      if (state.stopped) return;
      state.samples += 1;
      const ready = document.documentElement.hasAttribute("data-ccresdoc-load-controls-ready");
      if (!ready) {
        state.missingSince ??= performance.now();
        state.missingFrames += 1;
      } else if (state.missingSince !== null) {
        state.missingMs = Math.max(state.missingMs, performance.now() - state.missingSince);
        state.missingSince = null;
      }
      requestAnimationFrame(sample);
    };
    requestAnimationFrame(sample);
  });
  const target = page.locator('a[href*="/docs/hydration-pages/page-001/"]').first();
  await target.waitFor({ state: "visible", timeout: 5_000 });
  await target.click();
  await page.waitForFunction(() => location.pathname.endsWith("/docs/hydration-pages/page-001/"), undefined, { timeout: 15_000 });
  await page.waitForFunction(
    () => document.documentElement.hasAttribute("data-ccresdoc-load-controls-ready"),
    undefined,
    { timeout: 15_000 },
  );
  await delay(100);
  const transition = await page.evaluate(() => {
    const state = (window).__ccresdocHydrationSamples;
    state.stopped = true;
    if (state.missingSince !== null) state.missingMs = Math.max(state.missingMs, performance.now() - state.missingSince);
    return state;
  });
  assert(transition.missingMs <= 100, `pending treatment remained visible for ${transition.missingMs}ms`);
  assert(transition.missingFrames <= 8, `pending treatment spanned ${transition.missingFrames} animation frames`);

  const postSwapTheme = await page.locator("html").getAttribute("data-theme");
  const postSwapToggle = page.locator('[data-zfb-island="ThemeToggle"] button').first();
  await postSwapToggle.click();
  await page.waitForFunction(
    (before) => document.documentElement.getAttribute("data-theme") !== before,
    postSwapTheme,
    { timeout: 5_000 },
  );
  const postSwapLauncher = page.locator('[data-zfb-island="ThemePackSwitcher"] [data-switcher-launcher]').first();
  await postSwapLauncher.click();
  await page.locator('[data-zfb-island="ThemePackSwitcher"] [data-switcher-card][role="dialog"]').waitFor({ state: "visible", timeout: 5_000 });

  const finalModuleFailures = [...moduleRequests.values()].filter((record) => record.status === null || record.status < 200 || record.status >= 300);
  assert.equal(failedRequests.length, 0, `router swap produced failed module requests: ${JSON.stringify(failedRequests)}`);
  assert.equal(finalModuleFailures.length, 0, `router swap produced non-2xx module requests: ${JSON.stringify(finalModuleFailures)}`);
  assert.equal(mainDocumentRequests.length, 1, `router swap must not trigger a second document navigation: ${mainDocumentRequests}`);
  assertNoUnexpectedWorkspaceProcesses(workspace);
  await context.close();
  return {
    firstReady,
    samples: samples.length,
    mainDocumentRequests,
    moduleRequests: [...moduleRequests.values()],
    transition,
    workspace,
  };
}

async function main() {
  const packageJson = JSON.parse(readFileSync(join(appRoot, "package.json"), "utf8"));
  assert(packageJson.devDependencies?.playwright, "app/package.json must pin Playwright as a devDependency");
  assert.match(readFileSync(join(appRoot, "pnpm-lock.yaml"), "utf8"), /playwright(?:-core)?@1\.62\.1/);
  assertApplyReadinessContract();
  ensureStagedRuntime();
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  const probeRoot = mkdtempSync(join(tmpdir(), "ccresdoc-hydration-readiness-"));
  const workspace = join(probeRoot, "app-workspace");
  const sentinelDir = join(probeRoot, "sentinel-bin");
  const sentinelLog = join(probeRoot, "node-invocations.log");
  assertTemporaryWorkspace(workspace);
  mkdirSync(sentinelDir);
  writeFileSync(sentinelLog, "");
  const sentinel = join(sentinelDir, process.platform === "win32" ? "node.cmd" : "node");
  if (process.platform === "win32") {
    writeFileSync(sentinel, `@echo %*>>"${sentinelLog}"\r\n@exit /b 97\r\n`);
  } else {
    writeFileSync(sentinel, `#!/bin/sh\nprintf '%s\\n' "$*" >> '${sentinelLog.replaceAll("'", `\'"\'"\'`)}'\nexit 97\n`, { mode: 0o755 });
  }
  cpSync(stagedApp, workspace, { recursive: true, dereference: true });
  assertAllowlistedInventory(workspace);
  const privacy = assertRuntimeWorkspacePrivacy(workspace, { forbiddenPaths: [probeRoot, liveWorkspace] });
  const fixture = makeSyntheticFixture(workspace);
  const lock = acquirePortLock();
  let child;
  let browser;
  const samples = [];
  let signalShutdownStarted = false;
  const cleanupProbePaths = () => {
    rmSync(probeRoot, { recursive: true, force: true });
    rmSync(lock, { recursive: true, force: true });
  };
  process.once("exit", cleanupProbePaths);
  for (const [signal, exitCode] of [["SIGINT", 130], ["SIGTERM", 143]]) {
    process.once(signal, () => {
      if (signalShutdownStarted) return;
      signalShutdownStarted = true;
      void (async () => {
        try {
          if (browser) await browser.close().catch(() => {});
          if (child) await stopZfb(child, workspace).catch((error) => console.error(error));
        } finally {
          cleanupProbePaths();
          process.exit(exitCode);
        }
      })();
    });
  }
  try {
    await assertPortAvailable();
    const { webkit } = appRequire("playwright");
    try {
      browser = await webkit.launch();
    } catch (error) {
      throw new Error(`WebKit is not installed; run 'pnpm --dir app exec playwright install webkit': ${error.message}`);
    }
    const oraclePage = await browser.newPage();
    child = spawnZfb(workspace, sentinelDir, sentinelLog, manifest);
    const firstReady = await waitForHostReady(oraclePage, child, samples, workspace);
    assertRuntimeRenderedPrivacy("/docs/", (await fetchText("/docs/")).body, { forbiddenPaths: [probeRoot, liveWorkspace] });
    const browserResult = await assertBrowserSurface(browser, firstReady, samples, workspace);
    await oraclePage.close();
    await browser.close();
    browser = undefined;
    assert.equal(readFileSync(sentinelLog, "utf8"), "", "native zfb probe unexpectedly invoked Node");
    console.log(JSON.stringify({
      status: "passed",
      fixture: { syntheticPages: syntheticPageCount, files: fixture.files.length, root: relative(probeRoot, fixture.fixtureRoot) },
      samples: { count: samples.length, hostReady: samples.filter((sample) => sample.hostReady).length, firstReady: firstReady.at },
      browser: browserResult,
      privacy,
      webkitPrerequisite: "pnpm --dir app exec playwright install webkit",
      liveWorkspaceMutated: false,
    }, null, 2));
  } finally {
    if (browser) await browser.close().catch(() => {});
    if (child) {
      try {
        await stopZfb(child, workspace);
      } catch (error) {
        console.error(error);
        process.exitCode = 1;
      }
    }
    cleanupProbePaths();
  }
}

main().catch((error) => {
  console.error(error?.stack ?? error);
  process.exitCode = 1;
});
