#!/usr/bin/env node

/*
 * Deterministic browser-navigation confirmation for issue #221.
 *
 * The harness owns both ends of the test: it starts the installed native zfb
 * carrier on a freshly selected loopback port, waits for semantic `/docs/`
 * readiness, drives Chromium with real Playwright keyboard input, and tears
 * down the exact process group in every exit path.  It intentionally uses the
 * checked-in root, Claude, Codex, and 404 routes.  No selected local resource
 * directory or test fixture is copied into the app workspace.
 *
 * Run from the repository root after the app's frozen install:
 *
 *   pnpm --dir app exec playwright install chromium
 *   pnpm run test:browser-navigation
 */

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import { spawn } from "node:child_process";
import { createServer } from "node:net";
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const appRoot = join(repoRoot, "app");
const appRequire = createRequire(join(appRoot, "package.json"));
const semanticReadyTimeoutMs = 300_000;
const browserTimeoutMs = 30_000;
const ioTimeoutMs = 2_000;
const artifactDir = process.env.CCRESDOC_BROWSER_ARTIFACT_DIR
  ? resolve(process.env.CCRESDOC_BROWSER_ARTIFACT_DIR)
  : undefined;
let activeServerChild;
let activeBrowser;
let activeCleanupPromise;
const appRoutes = {
  root: "/docs/",
  claude: "/docs/claude/",
  codex: "/docs/codex/",
  missing: "/docs/browser-navigation-missing/",
  notFound: "/404.html",
};

const nativePackages = {
  "darwin-arm64": "@takazudo/zfb-darwin-arm64",
  "darwin-x64": "@takazudo/zfb-darwin-x64",
  "linux-arm64": "@takazudo/zfb-linux-arm64-gnu",
  "linux-x64": "@takazudo/zfb-linux-x64-gnu",
  "win32-x64": "@takazudo/zfb-win32-x64-msvc",
};

function usage() {
  console.log(`Usage: pnpm run test:browser-navigation [-- --headed|--contracts]

Starts a collision-safe native zfb dev server and runs the Chromium browser
navigation confirmation. Chromium must be installed once with:
  pnpm --dir app exec playwright install chromium

Options:
  --headed  Show Chromium while running the confirmation
  --contracts  Run repository/runtime contract checks without starting zfb or Chromium
  --help    Show this help

Set CCRESDOC_BROWSER_ARTIFACT_DIR to capture desktop, narrow, narrow-Find, and
desktop/narrow dark-mode review screenshots without changing the pass/fail
contract.`);
}

function parseOptions() {
  const args = process.argv.slice(2);
  if (args.includes("--help") || args.includes("-h")) {
    usage();
    process.exit(0);
  }
  return { headed: args.includes("--headed"), contracts: args.includes("--contracts") };
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function assertRepositoryContracts() {
  const rootPackage = readJson(join(repoRoot, "package.json"));
  assert.equal(
    rootPackage.scripts?.["test:browser-navigation"],
    "node scripts/confirm-browser-navigation.mjs",
    "the root browser-navigation script must remain the named harness entry",
  );

  const appPackage = readJson(join(appRoot, "package.json"));
  assert.equal(
    appPackage.devDependencies?.playwright,
    "1.62.1",
    "app/package.json must keep the pinned Playwright dependency",
  );
  assert.match(
    readFileSync(join(appRoot, "pnpm-lock.yaml"), "utf8"),
    /playwright(?:-core)?@1\.62\.1/,
    "app/pnpm-lock.yaml must keep the pinned Playwright resolution",
  );

  const catalog = readJson(join(appRoot, "src/browser-chrome/command-catalog.json"));
  const commands = new Map(catalog.commands.map((command) => [command.commandId, command]));
  for (const [commandId, binding] of [
    ["back", "Mod+["],
    ["forward", "Mod+]"],
    ["reload-documentation", "Mod+R"],
    ["find-in-page", "Mod+F"],
    ["search-documentation", "Mod+K"],
  ]) {
    assert(commands.has(commandId), `command catalog is missing ${commandId}`);
    assert(commands.get(commandId).defaultBindings.includes(binding), `${commandId} default binding drifted`);
  }

  const chromeSource = readFileSync(join(appRoot, "pages/lib/_chrome.ts"), "utf8");
  assert.match(chromeSource, /FindInPageInit,?\s*\{? disableBuiltInShortcut: true/s, "Find must opt out of the package shortcut");
  assert.match(chromeSource, /disableBuiltInShortcut:\s*true/, "Search must opt out of the package shortcut");

  // Test code is deliberately outside the staged app allowlist. These checks
  // make that privacy boundary visible to the browser gate itself.
  const runtimeFiles = readFileSync(join(repoRoot, "scripts/runtime-workspace-files.mjs"), "utf8");
  for (const required of [
    "patches/@takazudo__zudo-doc@5.12.1.patch",
    "src/browser-chrome/command-catalog.json",
    "src/browser-chrome/adapter.ts",
    "src/browser-chrome/history.ts",
    "src/browser-chrome/toolbar.tsx",
    "src/browser-chrome/types.ts",
  ]) {
    assert.match(runtimeFiles, new RegExp(`"${required.replaceAll(".", "\\.")}"`), `runtime allowlist is missing ${required}`);
  }
  assert.doesNotMatch(runtimeFiles, /confirm-browser-navigation|browser-navigation-contract/);
  for (const capability of ["default.json", "settings.json"]) {
    const manifest = readJson(join(repoRoot, "src-tauri/capabilities", capability));
    assert.deepEqual(
      manifest.windows,
      capability === "default.json" ? ["main"] : ["settings"],
      `${capability} must stay scoped to its one window`,
    );
    const permissions = manifest.permissions ?? [];
    assert(
      permissions.every((permission) => typeof permission === "string" && !/\*|allow-all|test-only|fixture/i.test(permission)),
      `${capability} contains a broad/test-only permission`,
    );
  }
  const generatedPermissions = join(repoRoot, "src-tauri/permissions/autogenerated");
  for (const file of readdirSync(generatedPermissions).filter((name) => name.endsWith(".toml"))) {
    const source = readFileSync(join(generatedPermissions, file), "utf8");
    assert.doesNotMatch(source, /\*|allow-all|test-only|fixture/i, `${file} contains a broad/test-only permission`);
  }
  const patch = join(appRoot, "patches/@takazudo__zudo-doc@5.12.1.patch");
  assert.equal(
    createHash("sha256").update(readFileSync(patch)).digest("hex"),
    "845bacae4edff6b516c1a26ac5d15d07ed4583f0dd908a883661be56463cbe53",
    "the controlled Find/Search patch bytes drifted",
  );
}

function nativeZfbPath() {
  const packageName = nativePackages[`${process.platform}-${process.arch}`];
  assert(packageName, `unsupported browser confirmation host: ${process.platform}-${process.arch}`);
  const binary = join(
    appRoot,
    "node_modules",
    ...packageName.split("/"),
    process.platform === "win32" ? "zfb.exe" : "zfb",
  );
  assert(existsSync(binary), `native zfb binary is missing: ${binary}; run pnpm --dir app install --frozen-lockfile`);
  if (process.platform !== "win32") {
    assert((statSync(binary).mode & 0o111) !== 0, `native zfb binary is not executable: ${binary}`);
  }
  return binary;
}

async function freeLoopbackPort() {
  const server = createServer();
  const port = await new Promise((resolvePort, rejectPort) => {
    server.once("error", rejectPort);
    server.listen({ host: "127.0.0.1", port: 0, exclusive: true }, () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        rejectPort(new Error("loopback port allocation returned no numeric address"));
        return;
      }
      resolvePort(address.port);
    });
  });
  await new Promise((resolveClose, rejectClose) => server.close((error) => error ? rejectClose(error) : resolveClose()));
  return port;
}

function appendOutput(child) {
  child.output = "";
  const append = (chunk) => {
    child.output += chunk.toString();
    if (child.output.length > 200_000) child.output = child.output.slice(-200_000);
  };
  child.stdout?.on("data", append);
  child.stderr?.on("data", append);
  child.exited = new Promise((resolveExit) => {
    let settled = false;
    const settle = (result) => {
      if (settled) return;
      settled = true;
      resolveExit(result);
    };
    child.once("exit", (code, signal) => settle({ code, signal }));
    child.once("error", (error) => {
      child.spawnError = error;
      append(error);
      settle({ code: null, signal: null, error });
    });
  });
  return child;
}

function signalProcessGroup(child, signal) {
  if (child?.pid === undefined || child.pid <= 0) return;
  if (process.platform === "win32") {
    child.kill(signal);
    return;
  }
  try {
    process.kill(-child.pid, signal);
  } catch (error) {
    if (error?.code !== "ESRCH") throw error;
  }
}

async function stopServer(child) {
  if (!child) return;
  if (child.exitCode === null) signalProcessGroup(child, "SIGTERM");
  let exited = await Promise.race([child.exited, delay(5_000).then(() => null)]);
  if (!exited) {
    signalProcessGroup(child, "SIGKILL");
    exited = await Promise.race([child.exited, delay(5_000).then(() => null)]);
  }
  assert(exited, `zfb process group ${child.pid} did not exit during harness cleanup`);
}

async function fetchText(url) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), ioTimeoutMs);
  try {
    const response = await fetch(url, {
      signal: controller.signal,
      headers: { connection: "close" },
    });
    return { status: response.status, body: await response.text() };
  } catch {
    return { status: 0, body: "" };
  } finally {
    clearTimeout(timer);
  }
}

function modulePaths(html, origin) {
  const paths = [];
  for (const match of html.matchAll(/<script\b([^>]*)>/gi)) {
    const attrs = match[1];
    const type = attrs.match(/\btype\s*=\s*["']?([^\s"'>]+)/i)?.[1]?.toLowerCase();
    if (type !== "module") continue;
    const raw = attrs.match(/\bsrc\s*=\s*["']([^"']+)["']/i)?.[1]?.trim();
    if (!raw || raw.includes("\r") || raw.includes("\n")) continue;
    let url;
    try { url = new URL(raw, origin); } catch { continue; }
    if (url.origin !== new URL(origin).origin || url.username || url.password) continue;
    paths.push(`${url.pathname}${url.search}`);
  }
  return [...new Set(paths)];
}

function semanticShellReady(body) {
  return body.includes("CCResDoc")
    && body.includes('data-ccresdoc-browser-toolbar-shell')
    && body.includes("/docs/claude/")
    && body.includes("/docs/codex/")
    && body.includes("data-zfb-island");
}

async function waitForSemanticReady(child, port) {
  const origin = `http://127.0.0.1:${port}`;
  const deadline = Date.now() + semanticReadyTimeoutMs;
  let last = { status: 0, body: "" };
  while (Date.now() < deadline) {
    if (child.spawnError) {
      throw new Error(`zfb failed to spawn: ${child.spawnError.message}`);
    }
    if (child.exitCode !== null || child.signalCode !== null) {
      throw new Error(`zfb exited before semantic readiness (code=${child.exitCode}, signal=${child.signalCode}):\n${child.output}`);
    }
    last = await fetchText(`${origin}${appRoutes.root}`);
    if (last.status === 200 && semanticShellReady(last.body)) {
      const modules = modulePaths(last.body, origin);
      if (modules.length > 0) {
        const statuses = await Promise.all(modules.map(async (path) => ({
          path,
          status: (await fetchText(`${origin}${path}`)).status,
        })));
        if (statuses.every(({ status }) => status >= 200 && status < 300)) {
          return { origin, modules, statuses };
        }
      }
    }
    await delay(100);
  }
  throw new Error(`timed out after ${semanticReadyTimeoutMs}ms waiting for semantic /docs/ readiness (last=${last.status})`);
}

async function startServer() {
  const binary = nativeZfbPath();
  let lastError;
  for (let attempt = 1; attempt <= 4; attempt += 1) {
    const port = await freeLoopbackPort();
    const env = { ...process.env };
    // The browser gate proves a real cold dev server, independently of any
    // inherited developer optimization or a stale build directory.
    delete env.ZFB_DEV_BOOT_LAZY;
    delete env.ZFB_DEV_EAGER;
    const child = appendOutput(spawn(binary, [
      "dev", "--host", "127.0.0.1", "--port", String(port),
    ], {
      cwd: appRoot,
      detached: process.platform !== "win32",
      env,
      stdio: ["ignore", "pipe", "pipe"],
    }));
    activeServerChild = child;
    try {
      const readiness = await waitForSemanticReady(child, port);
      return { child, port, ...readiness };
    } catch (error) {
      lastError = error;
      const collision = /address already in use|address in use|already bound|EADDRINUSE/i.test(child.output);
      await stopServer(child);
      if (activeServerChild === child) activeServerChild = undefined;
      if (!collision) throw error;
    }
  }
  throw new Error(`could not start zfb on a collision-safe loopback port: ${lastError?.message ?? "unknown error"}`);
}

async function closeActiveResources() {
  if (!activeCleanupPromise) {
    activeCleanupPromise = (async () => {
      const browser = activeBrowser;
      activeBrowser = undefined;
      if (browser) await browser.close().catch(() => {});
      const child = activeServerChild;
      activeServerChild = undefined;
      if (child) await stopServer(child);
    })();
  }
  return activeCleanupPromise;
}

function installSignalCleanup() {
  const onSignal = (signal) => {
    void (async () => {
      process.exitCode = signal === "SIGINT" ? 130 : 143;
      try {
        await closeActiveResources();
      } catch (error) {
        console.error(`cleanup after ${signal} failed:`, error?.stack ?? error);
        process.exitCode = 1;
      } finally {
        process.exit(process.exitCode);
      }
    })();
  };
  process.once("SIGINT", onSignal);
  process.once("SIGTERM", onSignal);
  return () => {
    process.off("SIGINT", onSignal);
    process.off("SIGTERM", onSignal);
  };
}

function t() {
  return {
    back: { commandId: "back", bindings: ["Mod+["] },
    forward: { commandId: "forward", bindings: ["Mod+]"] },
    home: { commandId: "home", bindings: [] },
    reload: { commandId: "reload-documentation", bindings: ["Mod+R"] },
    find: { commandId: "find-in-page", bindings: ["Mod+F"] },
    search: { commandId: "search-documentation", bindings: ["Mod+K"] },
    copy: { commandId: "copy-page-path", bindings: [] },
    open: { commandId: "open-in-default-browser", bindings: [] },
    settings: { commandId: "settings", bindings: [] },
  };
}

async function installTauriHarness(context) {
  await context.addInitScript(() => {
    const listeners = new Map();
    const bootstrap = {
      shortcutEntries: [
        { commandId: "back", bindings: ["Mod+["] },
        { commandId: "forward", bindings: ["Mod+]"] },
        { commandId: "home", bindings: [] },
        { commandId: "reload-documentation", bindings: ["Mod+R"] },
        { commandId: "find-in-page", bindings: ["Mod+F"] },
        { commandId: "search-documentation", bindings: ["Mod+K"] },
        { commandId: "copy-page-path", bindings: [] },
        { commandId: "open-in-default-browser", bindings: [] },
        { commandId: "settings", bindings: [] },
      ],
      nativeOwnedBindings: [],
      hostCapabilities: { reloadDocumentation: true, openInDefaultBrowser: true },
      runtimeGeneration: 1,
    };
    window.__TAURI_INTERNALS__ = {};
    window.__ccresdocTauriCalls = [];
    window.__ccresdocEmitBootstrap = (payload) => {
      for (const listener of listeners.get("ccresdoc://browser-bootstrap") ?? []) listener({ payload });
    };
    let appearance = { mode: "system", themePack: "default" };
    window.__TAURI__ = {
      core: {
        invoke: async (command, args) => {
          window.__ccresdocTauriCalls.push({ command, args });
          if (command === "get_browser_bootstrap") return bootstrap;
          // AppearanceBridge routes every theme change through update_appearance and
          // reverts the DOM to the last authoritative value when the call rejects, so
          // a stub returning undefined makes the theme toggle silently snap back.
          // Shape mirrors Rust's AppearanceEnvelope (src-tauri/src/appearance.rs).
          if (command === "update_appearance") {
            const request = args?.request ?? {};
            appearance = {
              mode: request.mode ?? appearance.mode,
              themePack: request.themePack ?? appearance.themePack,
            };
            return {
              appearance: { ...appearance },
              authoritative: { ...appearance },
              revision: null,
              source: "authoritative",
              authoritativeSource: "authoritative",
            };
          }
          return undefined;
        },
      },
      event: {
        listen: async (event, listener) => {
          const current = listeners.get(event) ?? [];
          current.push(listener);
          listeners.set(event, current);
          return () => listeners.set(event, current.filter((candidate) => candidate !== listener));
        },
      },
    };
  });
}

async function waitForToolbar(page) {
  await page.waitForFunction(() => {
    const toolbar = document.querySelector("nav.ccresdoc-browser-toolbar");
    return toolbar?.getAttribute("data-bootstrap") === "ready"
      && toolbar.querySelector('[aria-label="Current documentation path"]') !== null;
  }, undefined, { timeout: browserTimeoutMs });
}

async function waitForPath(page, path) {
  await page.waitForFunction((expected) => location.pathname === expected, path, { timeout: browserTimeoutMs });
  await page.waitForFunction((expected) => (
    document.querySelector('[aria-label="Current documentation path"]')?.value === expected
  ), path, { timeout: browserTimeoutMs });
}

async function openPage(page, origin, path) {
  await page.goto(`${origin}${path}`, { waitUntil: "domcontentloaded", timeout: browserTimeoutMs });
  await waitForToolbar(page);
  await waitForPath(page, path);
  await page.waitForFunction(
    () => document.documentElement.hasAttribute("data-ccresdoc-load-controls-ready"),
    undefined,
    { timeout: browserTimeoutMs },
  );
}

function toolbar(page) {
  return page.locator("nav.ccresdoc-browser-toolbar");
}

function command(page, commandId) {
  return toolbar(page).locator(`[data-browser-command="${commandId}"]`).first();
}

async function waitForCommandEnabled(page, commandId) {
  await page.waitForFunction((id) => (
    document.querySelector(`[data-browser-command="${id}"]`)?.disabled === false
  ), commandId, { timeout: browserTimeoutMs });
}

function menu(page) {
  return toolbar(page).locator('[role="menu"]');
}

async function assertClipboard(page, expected, message) {
  await page.waitForFunction(async (value) => await navigator.clipboard.readText() === value, expected, { timeout: browserTimeoutMs });
  assert.equal(await page.evaluate(() => navigator.clipboard.readText()), expected, message);
}

async function routeViaHeader(page, path) {
  const link = page.locator(`header a[href$="${path}"]`).first();
  await link.waitFor({ state: "visible", timeout: browserTimeoutMs });
  await link.click();
  await waitForPath(page, path);
  await waitForToolbar(page);
}

async function managedHistory(page) {
  return page.evaluate(() => {
    const tagged = history.state?.__ccresdocBrowserHistoryV1;
    const scope = tagged?.scope;
    let stored = null;
    if (scope) {
      try { stored = JSON.parse(sessionStorage.getItem(`ccresdoc:browser-history:v1:${scope}`) ?? "null"); } catch {}
    }
    return {
      routerIndex: history.state?.index,
      tagged,
      stored,
      href: location.href,
    };
  });
}

async function closeFind(page) {
  const bar = page.locator("[data-find-in-page-bar]");
  if (await bar.count()) {
    await page.keyboard.press("Escape");
    await page.waitForFunction(() => document.querySelector("[data-find-in-page-bar]") === null, undefined, { timeout: browserTimeoutMs });
  }
  await page.waitForFunction(() => document.querySelectorAll("[data-find-match]").length === 0, undefined, { timeout: browserTimeoutMs });
}

async function closeSearch(page) {
  const dialog = page.locator("dialog[data-search-dialog]");
  if (await dialog.count() && await dialog.evaluate((node) => node.open)) {
    await page.keyboard.press("Escape");
    await page.waitForFunction(() => !document.querySelector("dialog[data-search-dialog]")?.open, undefined, { timeout: browserTimeoutMs });
  }
}

async function openPatchedFind(page, byKeyboard = false) {
  await closeFind(page);
  if (byKeyboard) await page.keyboard.press("Control+F");
  else await command(page, "find-in-page").click();
  await page.waitForFunction(() => document.querySelector("[data-find-in-page-bar]") !== null, undefined, { timeout: browserTimeoutMs });
  await page.waitForFunction(() => (
    document.querySelector('[data-find-in-page-bar] input[aria-label="Find in page"]')?.matches(":focus") === true
  ), undefined, { timeout: browserTimeoutMs });
  return page.locator('[data-find-in-page-bar] input[aria-label="Find in page"]');
}

async function openControlledSearch(page, byKeyboard = false) {
  await closeSearch(page);
  if (byKeyboard) {
    await page.evaluate(() => {
      window.__ccresdocSearchKeys = [];
      window.addEventListener("keydown", (event) => {
        const record = {
          key: event.key,
          ctrlKey: event.ctrlKey,
          target: event.target?.outerHTML,
          active: document.activeElement?.outerHTML,
          defaultPrevented: event.defaultPrevented,
        };
        window.__ccresdocSearchKeys.push(record);
        queueMicrotask(() => { record.defaultPrevented = event.defaultPrevented; });
      }, { capture: true, once: true });
    });
    await page.keyboard.press("Control+K");
  } else {
    await command(page, "more").click();
    await menu(page).waitFor({ state: "visible", timeout: browserTimeoutMs });
    await menu(page).locator('[data-browser-command="search-documentation"]').click();
  }
  try {
    await page.waitForFunction(() => document.querySelector("dialog[data-search-dialog]")?.open === true, undefined, { timeout: browserTimeoutMs });
  } catch (error) {
    const diagnostic = await page.evaluate(() => ({
      path: location.pathname,
      active: document.activeElement?.outerHTML,
      dialogOpen: document.querySelector("dialog[data-search-dialog]")?.open,
      toolbarBootstrap: document.querySelector("nav.ccresdoc-browser-toolbar")?.getAttribute("data-bootstrap"),
      keys: window.__ccresdocSearchKeys,
    }));
    throw new Error(`Search did not open: ${JSON.stringify(diagnostic)}`, { cause: error });
  }
  return page.locator("dialog[data-search-dialog] [data-search-input]");
}

async function activeFindIndex(page) {
  return page.evaluate(() => [...document.querySelectorAll("[data-find-match]")].findIndex((mark) => mark.hasAttribute("data-find-active")));
}

async function assertFindSurface(page) {
  // The root fixture intentionally contains CCResDoc in both its rendered
  // heading and body. Earlier history/reload checks finish on a category page,
  // whose article has only one occurrence, so establish the deterministic
  // Find fixture explicitly before asserting traversal between matches.
  await openPage(page, new URL(page.url()).origin, appRoutes.root);
  const toolbarFind = await openPatchedFind(page);
  await toolbarFind.fill("CCResDoc");
  try {
    await page.waitForFunction(() => document.querySelectorAll("[data-find-match]").length >= 2, undefined, { timeout: browserTimeoutMs });
  } catch (error) {
    const diagnostic = await page.evaluate(() => ({
      path: location.pathname,
      input: document.querySelector('[data-find-in-page-bar] input[aria-label="Find in page"]')?.value,
      marks: document.querySelectorAll("[data-find-match]").length,
      articleText: document.querySelector("article.zd-content")?.textContent,
    }));
    throw new Error(`Find did not mark the deterministic root fixture: ${JSON.stringify(diagnostic)}`, { cause: error });
  }
  const marks = await page.locator("[data-find-match]").count();
  assert(marks >= 2, `expected deterministic CCResDoc content to produce at least two matches, got ${marks}`);
  assert.equal(await activeFindIndex(page), 0, "Find starts at the first match");
  await toolbarFind.press("Enter");
  const next = await activeFindIndex(page);
  assert.notEqual(next, 0, "Enter advances the active Find match");
  await toolbarFind.press("Shift+Enter");
  assert.equal(await activeFindIndex(page), 0, "Shift+Enter returns to the previous Find match");
  await page.locator('[data-find-in-page-bar] button[aria-label="Next match (Enter)"]').click();
  assert.notEqual(await activeFindIndex(page), 0, "Find next icon advances the active match");
  await page.locator('[data-find-in-page-bar] button[aria-label="Previous match (Shift+Enter)"]').click();
  assert.equal(await activeFindIndex(page), 0, "Find previous icon returns to the prior match");
  await toolbarFind.fill("ccresdoc-no-match-221");
  await page.waitForFunction(() => document.querySelectorAll("[data-find-match]").length === 0, undefined, { timeout: browserTimeoutMs });
  await closeFind(page);

  const keyboardFind = await openPatchedFind(page, true);
  assert.equal(await keyboardFind.count(), 1, "actual Control+F opens the patched Find input");
  await closeFind(page);

  // A route swap must clear both marks and the bar, even when the swap starts
  // while the Find input owns focus.
  const routeFind = await openPatchedFind(page);
  await routeFind.fill("CCResDoc");
  await page.waitForFunction(() => document.querySelectorAll("[data-find-match]").length >= 2, undefined, { timeout: browserTimeoutMs });
  await routeViaHeader(page, appRoutes.claude);
  await page.waitForFunction(() => document.querySelector("[data-find-in-page-bar]") === null, undefined, { timeout: browserTimeoutMs });
  assert.equal(await page.locator("[data-find-match]").count(), 0, "route swap clears Find marks");
}

async function assertSearchSurface(page) {
  await page.evaluate(() => {
    const nativeFetch = window.fetch.bind(window);
    window.__ccresdocSearchFetch = { nativeFetch: window.fetch, urls: [] };
    window.fetch = (input, init) => {
      const raw = input instanceof Request ? input.url : String(input);
      window.__ccresdocSearchFetch.urls.push(new URL(raw, location.href).href);
      return nativeFetch(input, init);
    };
  });
  try {
    const toolbarInput = await openControlledSearch(page);
    assert.equal(await toolbarInput.count(), 1, "toolbar Search opens the package widget");
    await page.waitForFunction(() => document.querySelector("dialog[data-search-dialog] [data-search-input]")?.matches(":focus"), undefined, { timeout: browserTimeoutMs });
    await closeSearch(page);
    const keyboardInput = await openControlledSearch(page, true);
    assert.equal(await keyboardInput.count(), 1, "actual Control+K opens the package widget");
    try {
      await page.waitForFunction(() => window.__ccresdocSearchFetch.urls.some((raw) => (
        new URL(raw).pathname === "/docs/search-index.json"
      )), undefined, { timeout: browserTimeoutMs });
    } catch (error) {
      const fetchUrls = await page.evaluate(() => window.__ccresdocSearchFetch.urls);
      throw new Error(`Search did not fetch the public index: ${JSON.stringify(fetchUrls)}`, { cause: error });
    }
    await closeSearch(page);
    const fetchUrls = await page.evaluate(() => window.__ccresdocSearchFetch.urls);
    assert(
      fetchUrls.some((raw) => new URL(raw).pathname === "/docs/search-index.json"),
      `controlled Search must refresh its public search-index endpoint: ${JSON.stringify(fetchUrls)}`,
    );
  } finally {
    await page.evaluate(() => {
      if (window.__ccresdocSearchFetch?.nativeFetch) window.fetch = window.__ccresdocSearchFetch.nativeFetch;
      delete window.__ccresdocSearchFetch;
    });
  }
}

async function assertEditingTargetSuppression(page) {
  await openPage(page, new URL(page.url()).origin, appRoutes.root);
  await page.evaluate(() => {
    const host = document.createElement("div");
    host.id = "browser-navigation-editing-targets-221";
    host.innerHTML = `
      <input data-browser-navigation-target="input" aria-label="editing target">
      <textarea data-browser-navigation-target="textarea" aria-label="editing target"></textarea>
      <select data-browser-navigation-target="select" aria-label="editing target"><option>editing target</option></select>
      <div data-browser-navigation-target="contenteditable" contenteditable="true" role="textbox" tabindex="0"></div>
      <input data-browser-navigation-target="capture" data-shortcut-capture aria-label="shortcut capture">
    `;
    host.style.cssText = "position:fixed;inset-block-start:4px;inset-inline-start:4px;z-index:9999";
    document.body.append(host);
  });
  try {
    for (const target of ["input", "textarea", "select", "contenteditable", "capture"]) {
      await closeFind(page);
      await closeSearch(page);
      await page.locator(`[data-browser-navigation-target="${target}"]`).focus();
      await page.keyboard.press("Control+F");
      await page.keyboard.press("Control+K");
      await delay(80);
      assert.equal(await page.locator("[data-find-in-page-bar]").count(), 0, `${target} must suppress Find shortcut dispatch`);
      assert.equal(await page.locator("dialog[data-search-dialog][open]").count(), 0, `${target} must suppress Search shortcut dispatch`);
    }

    const searchInput = await openControlledSearch(page);
    await searchInput.focus();
    await page.keyboard.press("Control+F");
    await page.keyboard.press("Control+K");
    await delay(80);
    assert.equal(await page.locator("[data-find-in-page-bar]").count(), 0, "site Search input must suppress Find shortcut dispatch");
    assert.equal(await page.locator("dialog[data-search-dialog][open]").count(), 1, "site Search input must retain its own dialog without duplicate dispatch");
    await closeSearch(page);

    const findInput = await openPatchedFind(page);
    await findInput.focus();
    await page.keyboard.press("Control+F");
    await page.keyboard.press("Control+K");
    await delay(80);
    assert.equal(await page.locator("[data-find-in-page-bar]").count(), 1, "Find input must suppress duplicate Find shortcut dispatch");
    assert.equal(await page.locator("dialog[data-search-dialog][open]").count(), 0, "Find input must suppress Search shortcut dispatch");
    await closeFind(page);
  } finally {
    await page.evaluate(() => document.querySelector("#browser-navigation-editing-targets-221")?.remove());
  }
}

async function assertHistorySurface(page, origin) {
  await openPage(page, origin, appRoutes.root);
  await page.evaluate(() => {
    const shell = document.querySelector("[data-ccresdoc-browser-toolbar-shell]");
    window.__ccresdocHistoryTrace = { shell, events: [] };
    for (const name of ["zfb:before-preparation", "zfb:after-swap", "zfb:page-load", "zfb:navigation-aborted", "popstate"]) {
      const target = name === "popstate" ? window : document;
      target.addEventListener(name, (event) => window.__ccresdocHistoryTrace.events.push({
        name,
        navigationType: event.navigationType,
        path: location.pathname,
        state: structuredClone(history.state),
      }));
    }
  });
  const first = await managedHistory(page);
  assert(first.stored, "initial managed route must persist a browser-history record");
  assert.equal(first.stored?.boundary, first.stored?.current, "initial managed route starts at its boundary");
  assert.equal(await command(page, "back").isDisabled(), true, "Back is disabled at the first managed route");
  const navigations = [];
  const onFrameNavigated = (frame) => { if (frame === page.mainFrame()) navigations.push(frame.url()); };
  page.on("framenavigated", onFrameNavigated);
  try {
    await Promise.allSettled([command(page, "back").click(), command(page, "back").click()]);
    await waitForPath(page, appRoutes.root);
    assert(navigations.every((url) => url.startsWith(origin)), `Back exposed a foreign/loading navigation: ${navigations}`);

    await routeViaHeader(page, appRoutes.claude);
    await routeViaHeader(page, appRoutes.codex);
    const routedHistory = await managedHistory(page);
    const historyTrace = await page.evaluate(() => ({
      shellPersisted: window.__ccresdocHistoryTrace.shell === document.querySelector("[data-ccresdoc-browser-toolbar-shell]"),
      events: window.__ccresdocHistoryTrace.events,
    }));
    assert.equal(
      await command(page, "back").isDisabled(),
      false,
      `Back must enable after two pushes: ${JSON.stringify({ routedHistory, historyTrace })}`,
    );
    await page.keyboard.press("Control+[");
    await waitForPath(page, appRoutes.claude);
    // popstate updates the URL/path before zfb:page-load settles the managed
    // traversal. Wait for the exposed Forward affordance instead of racing
    // that intentional traversal lock with the opposite shortcut.
    await waitForCommandEnabled(page, "forward");
    const beforeForward = await managedHistory(page);
    const forwardDisabled = await command(page, "forward").isDisabled();
    await page.evaluate(() => {
      window.__ccresdocForwardKeys = [];
      window.addEventListener("keydown", (event) => {
        const record = {
          key: event.key,
          code: event.code,
          ctrlKey: event.ctrlKey,
          metaKey: event.metaKey,
          target: event.target?.tagName,
          active: document.activeElement?.tagName,
          defaultPrevented: event.defaultPrevented,
        };
        window.__ccresdocForwardKeys.push(record);
        queueMicrotask(() => { record.defaultPrevented = event.defaultPrevented; });
      }, true);
    });
    await page.keyboard.press("Control+]");
    try {
      await page.waitForFunction((expected) => location.pathname === expected, appRoutes.codex, { timeout: 3_000 });
      await waitForPath(page, appRoutes.codex);
      await waitForCommandEnabled(page, "back");
    } catch (error) {
      const diagnostic = await page.evaluate(() => ({
        path: location.pathname,
        active: document.activeElement?.outerHTML,
        forwardDisabled: document.querySelector('[data-browser-command="forward"]')?.disabled,
        keys: window.__ccresdocForwardKeys,
      }));
      throw new Error(`Forward shortcut did not traverse: ${JSON.stringify({ beforeForward, forwardDisabled, diagnostic, history: await managedHistory(page) })}`, { cause: error });
    }

    // C → Back to B → D (Home) creates a new branch. Forward must remain
    // disabled, and the old C entry must not become reachable again.
    await page.keyboard.press("Control+[");
    await waitForPath(page, appRoutes.claude);
    await waitForCommandEnabled(page, "forward");
    await command(page, "home").click();
    await waitForPath(page, appRoutes.root);
    assert.equal(await command(page, "forward").isDisabled(), true, "Forward is disabled after a new branch");
    const branchPath = page.url();
    await Promise.allSettled([command(page, "forward").click(), page.keyboard.press("Control+]")]);
    await delay(100);
    assert.equal(page.url(), branchPath, "the superseded C entry is unreachable after branching");

    // A second generation starts a fresh managed boundary at the current
    // route; it never inherits a Back affordance from the old runtime.
    await page.evaluate(() => window.__ccresdocEmitBootstrap({
      shortcutEntries: [
        { commandId: "back", bindings: ["Mod+["] },
        { commandId: "forward", bindings: ["Mod+]"] },
        { commandId: "home", bindings: [] },
        { commandId: "reload-documentation", bindings: ["Mod+R"] },
        { commandId: "find-in-page", bindings: ["Mod+F"] },
        { commandId: "search-documentation", bindings: ["Mod+K"] },
        { commandId: "copy-page-path", bindings: [] },
        { commandId: "open-in-default-browser", bindings: [] },
        { commandId: "settings", bindings: [] },
      ],
      nativeOwnedBindings: [],
      hostCapabilities: { reloadDocumentation: true, openInDefaultBrowser: true },
      runtimeGeneration: 221,
    }));
    await page.waitForFunction(() => document.querySelector("nav.ccresdoc-browser-toolbar")?.getAttribute("data-bootstrap") === "ready", undefined, { timeout: browserTimeoutMs });
    const fresh = await managedHistory(page);
    assert(fresh.stored, "new runtime generation must persist its managed history record");
    assert.equal(fresh.stored?.boundary, fresh.stored?.current, "new runtime generation establishes a fresh boundary");
    assert.equal(await command(page, "back").isDisabled(), true, "Back is disabled at a new runtime boundary");
  } finally {
    page.off("framenavigated", onFrameNavigated);
  }
}

async function assertReloadAndPageshow(page, origin) {
  await openPage(page, origin, appRoutes.codex);
  await routeViaHeader(page, appRoutes.claude);
  const before = await managedHistory(page);
  assert(before.stored, "deep route must retain a managed history record before reload");
  const boundary = before.stored?.boundary;
  await page.reload({ waitUntil: "domcontentloaded", timeout: browserTimeoutMs });
  await waitForToolbar(page);
  await waitForPath(page, appRoutes.claude);
  const afterReload = await managedHistory(page);
  assert.equal(afterReload.stored?.boundary, boundary, "hard reload preserves the managed boundary");
  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent("pageshow", { persisted: true })));
  const afterRestore = await managedHistory(page);
  assert.equal(afterRestore.stored?.boundary, boundary, "pageshow restore preserves the managed boundary");
  const exposedLoadingDocument = await page.locator("body").evaluate(
    (body) => body.textContent?.includes("CCResDoc is starting") ?? false,
  );
  assert(!exposedLoadingDocument, "loading document was not exposed during deep-route restore");
}

async function assertShortcutReconfiguration(page) {
  const calls = await page.evaluate(() => window.__ccresdocTauriCalls);
  assert(Array.isArray(calls), "Tauri harness did not expose invocation evidence");
  await page.evaluate(() => {
    window.__ccresdocCommandEvents = { find: 0, search: 0 };
    document.addEventListener("zudo-doc:find-in-page-command", () => { window.__ccresdocCommandEvents.find += 1; });
    document.addEventListener("zudo-doc:search-command", () => { window.__ccresdocCommandEvents.search += 1; });
  });

  const customEntries = Object.values(t()).map((entry) => ({ ...entry, bindings: entry.commandId === "find-in-page"
    ? ["Mod+Shift+F"]
    : entry.commandId === "search-documentation" ? ["Mod+Shift+K"] : entry.bindings }));
  await page.evaluate((shortcutEntries) => window.__ccresdocEmitBootstrap({
    shortcutEntries,
    nativeOwnedBindings: [],
    hostCapabilities: { reloadDocumentation: true, openInDefaultBrowser: true },
    runtimeGeneration: 222,
  }), customEntries);
  await delay(50);
  await closeFind(page);
  await closeSearch(page);
  await page.keyboard.press("Control+F");
  await page.keyboard.press("Control+K");
  await delay(100);
  assert.equal(await page.locator("[data-find-in-page-bar]").count(), 0, "changed Mod+F no longer opens the stale Find default");
  assert.equal(await page.locator("dialog[data-search-dialog][open]").count(), 0, "changed Mod+K no longer opens the stale Search default");
  await page.keyboard.press("Control+Shift+F");
  await page.waitForFunction(() => document.querySelector("[data-find-in-page-bar]") !== null, undefined, { timeout: browserTimeoutMs });
  const customFind = page.locator('[data-find-in-page-bar] input[aria-label="Find in page"]');
  assert.equal(await customFind.count(), 1, "custom secondary Find binding opens with actual key input");
  assert.equal(await page.evaluate(() => window.__ccresdocCommandEvents.find), 1, "custom Find binding dispatches exactly once");
  await closeFind(page);
  await page.keyboard.press("Control+Shift+K");
  await page.waitForFunction(() => document.querySelector("dialog[data-search-dialog]")?.open === true, undefined, { timeout: browserTimeoutMs });
  assert.equal(await page.evaluate(() => window.__ccresdocCommandEvents.search), 1, "custom Search binding dispatches exactly once");
  await closeSearch(page);

  const removedEntries = Object.values(t()).map((entry) => ({ ...entry, bindings: [] }));
  await page.evaluate((shortcutEntries) => window.__ccresdocEmitBootstrap({
    shortcutEntries,
    nativeOwnedBindings: [],
    hostCapabilities: { reloadDocumentation: true, openInDefaultBrowser: true },
    runtimeGeneration: 223,
  }), removedEntries);
  await delay(50);
  await page.keyboard.press("Control+F");
  await page.keyboard.press("Control+K");
  await delay(100);
  assert.equal(await page.locator("[data-find-in-page-bar]").count(), 0, "removed Mod+F does not leave a package listener behind");
  assert.equal(await page.locator("dialog[data-search-dialog][open]").count(), 0, "removed Mod+K does not leave a package listener behind");
  assert.deepEqual(await page.evaluate(() => window.__ccresdocCommandEvents), { find: 1, search: 1 }, "removed bindings do not dispatch stale package commands");
}

async function assertMoreAndToolbarActions(page) {
  await openPage(page, new URL(page.url()).origin, appRoutes.root);
  await closeFind(page);
  await closeSearch(page);
  await command(page, "copy-page-path").click();
  await assertClipboard(page, appRoutes.root, "toolbar Copy copies the managed path");
  const more = command(page, "more");
  const trigger = more;
  await trigger.click();
  assert.equal(await menu(page).isVisible(), true, "mouse opens More");
  await page.keyboard.press("Escape");
  assert.equal(await menu(page).isVisible(), false, "Escape closes More");
  assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("data-browser-command")), "more", "Escape restores More focus");

  await trigger.focus();
  await page.keyboard.press("Enter");
  assert.equal(await menu(page).isVisible(), true, "Enter opens More");
  await page.keyboard.press("Escape");
  await trigger.focus();
  await page.keyboard.press(" ");
  assert.equal(await menu(page).isVisible(), true, "Space opens More");
  await page.keyboard.press("Escape");
  await trigger.focus();
  await page.keyboard.press("ArrowDown");
  assert.equal(await menu(page).isVisible(), true, "ArrowDown opens More");
  assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("data-browser-command")), "search-documentation", "ArrowDown focuses the first menu action");
  await page.keyboard.press("End");
  assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("data-browser-command")), "settings", "End focuses the last menu action");
  await page.keyboard.press("Home");
  assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("data-browser-command")), "search-documentation", "Home focuses the first menu action");
  await page.getByRole("heading", { name: "CCResDoc Resources", exact: true }).click();
  await menu(page).waitFor({ state: "hidden", timeout: browserTimeoutMs });
  assert.equal(await menu(page).isVisible(), false, "outside click closes More");

  await trigger.click();
  await menu(page).locator('[data-browser-command="copy-page-path"]').click();
  await assertClipboard(page, appRoutes.root, "Copy copies the managed path");
  await trigger.click();
  await menu(page).locator('[data-browser-command="open-in-default-browser"]').click();
  await trigger.click();
  await menu(page).locator('[data-browser-command="settings"]').click();
  const invoked = await page.evaluate(() => window.__ccresdocTauriCalls.map(({ command }) => command));
  assert(invoked.includes("open_current_page_in_default_browser"), "More exposes the host external-open action");
  assert(invoked.includes("open_settings_window"), "More exposes the host Settings action");
  await command(page, "reload-documentation").click();
  const afterReloadCommand = await page.evaluate(() => window.__ccresdocTauriCalls.some(({ command }) => command === "reload_documentation"));
  assert(afterReloadCommand, "toolbar Reload Documentation uses the host command seam");
}

async function geometry(page) {
  return page.evaluate(() => {
    const root = document.querySelector("nav.ccresdoc-browser-toolbar");
    const leading = root?.querySelector(".ccresdoc-browser-toolbar__leading");
    const trailing = root?.querySelector(".ccresdoc-browser-toolbar__trailing");
    const path = root?.querySelector(".ccresdoc-browser-toolbar__path");
    const button = root?.querySelector('[data-browser-command="back"]');
    const menuRow = root?.querySelector('[role="menuitem"]');
    const rect = (element) => element?.getBoundingClientRect().toJSON() ?? null;
    return {
      toolbar: rect(root),
      leading: rect(leading),
      trailing: rect(trailing),
      path: rect(path),
      button: rect(button),
      menuRow: rect(menuRow),
      leadingText: [...(leading?.querySelectorAll("button") ?? [])].map((item) => item.textContent?.trim() ?? ""),
      mediaCoarse: matchMedia("(pointer: coarse)").matches,
    };
  });
}

async function assertResponsiveGeometry(page, origin) {
  await openPage(page, origin, appRoutes.root);
  if (artifactDir) await page.screenshot({ path: join(artifactDir, "browser-toolbar-desktop.png") });
  await command(page, "more").click();
  const desktopMenu = menu(page);
  const desktop = await geometry(page);
  assert(Math.abs(desktop.toolbar.height - 52) <= 1, `desktop toolbar must be 52px, got ${desktop.toolbar.height}`);
  assert(Math.abs(desktop.button.width - 36) <= 1 && Math.abs(desktop.button.height - 36) <= 1, `desktop targets must be 36px, got ${JSON.stringify(desktop.button)}`);
  assert(desktop.path.width > 500, "desktop path must consume the remaining row width");
  assert(desktop.path.left >= desktop.leading.right - 1 && desktop.path.right <= desktop.trailing.left + 1, "path must sit between complete leading/trailing action groups");
  assert(desktop.leadingText.every((text) => text === ""), "leading actions must remain bare icons");
  assert(await desktopMenu.isVisible(), "desktop More menu is reachable");
  await page.keyboard.press("Escape");
  await page.setViewportSize({ width: 390, height: 844 });
  await delay(50);
  if (artifactDir) await page.screenshot({ path: join(artifactDir, "browser-toolbar-narrow.png") });
  const narrow = await geometry(page);
  assert(Math.abs(narrow.toolbar.height - 52) <= 1, `narrow toolbar must be 52px, got ${narrow.toolbar.height}`);
  assert(narrow.path.width > 0, "narrow path must retain all available width");
  assert.equal(await command(page, "back").isVisible(), true, "narrow Back remains direct");
  assert.equal(await command(page, "forward").isVisible(), true, "narrow Forward remains direct");
  assert.equal(await command(page, "find-in-page").isVisible(), true, "narrow Find remains direct");
  assert.equal(await command(page, "copy-page-path").isVisible(), false, "narrow Copy moves to More");
  assert.equal(await command(page, "open-in-default-browser").isVisible(), false, "narrow external-open moves to More");
  await command(page, "find-in-page").click();
  if (artifactDir) await page.screenshot({ path: join(artifactDir, "browser-toolbar-narrow-find.png") });
  const findBar = page.locator("[data-find-in-page-bar]");
  const barBox = await findBar.boundingBox();
  const toolbarBox = await toolbar(page).boundingBox();
  const headerBox = await page.locator("header[data-header]").boundingBox();
  assert(barBox && toolbarBox, "narrow Find bar and toolbar must have geometry");
  const lowerChrome = toolbarBox.y + toolbarBox.height + (headerBox?.height ?? 0);
  assert(barBox.y >= lowerChrome - 2, `Find bar overlaps sticky chrome: bar=${barBox.y}, lowerChrome=${lowerChrome}`);
  await closeFind(page);
  await page.setViewportSize({ width: 1280, height: 900 });
}

// Resolves a CSS length (including a var()/calc() custom property) to a
// pixel number the way the layout engine actually resolves it, by reading
// back the computed style of a throwaway probe element instead of
// re-deriving the arithmetic in JS. Used by the chrome-offset assertions
// below so they track the real cascade rather than a second hardcoded copy
// of it.
async function chromeOffsetPx(page) {
  return page.evaluate(() => {
    const probe = document.createElement("div");
    probe.style.cssText = "position:fixed;visibility:hidden;left:0;top:var(--ccresdoc-complete-chrome-offset)";
    document.body.append(probe);
    const px = parseFloat(getComputedStyle(probe).top);
    probe.remove();
    return px;
  });
}

async function assertStickyChromeRegion(page, origin) {
  await openPage(page, origin, appRoutes.root);
  const region = page.locator("[data-ccresdoc-chrome-region]");
  const position = await region.evaluate((element) => getComputedStyle(element).position);
  assert.equal(position, "sticky", "the chrome region wrapper must use sticky positioning");

  // The checked-in landing routes are too short to scroll on their own, so a
  // spacer is injected to make the assertion meaningful; without it
  // window.scrollY would stay 0 and the geometry comparison below would pass
  // vacuously regardless of whether sticky positioning actually works.
  await page.evaluate(() => {
    const spacer = document.createElement("div");
    spacer.id = "browser-navigation-scroll-spacer-232";
    spacer.style.cssText = "block-size:3000px";
    document.body.append(spacer);
  });
  try {
    const beforeScrollY = await page.evaluate(() => window.scrollY);
    await page.evaluate(() => window.scrollTo(0, 1500));
    await page.waitForFunction((before) => window.scrollY > before, beforeScrollY, { timeout: browserTimeoutMs });
    const afterScrollY = await page.evaluate(() => window.scrollY);
    assert(afterScrollY > beforeScrollY, `the scroll must actually move the viewport before geometry is compared: before=${beforeScrollY}, after=${afterScrollY}`);
    const box = await region.boundingBox();
    assert(box, "the chrome region must have geometry after scrolling");
    assert(Math.abs(box.y) <= 1, `the sticky chrome region must stay pinned to the viewport top after scrolling, got y=${box.y}`);
  } finally {
    await page.evaluate(() => {
      document.querySelector("#browser-navigation-scroll-spacer-232")?.remove();
      window.scrollTo(0, 0);
    });
  }
}

async function assertChromeAdjacency(page, origin) {
  await openPage(page, origin, appRoutes.root);
  const toolbarBox = await toolbar(page).boundingBox();
  const headerBox = await page.locator("header[data-header]").boundingBox();
  assert(toolbarBox && headerBox, "toolbar and header must have geometry");
  const gap = headerBox.y - (toolbarBox.y + toolbarBox.height);
  assert(Math.abs(gap) <= 1, `toolbar and header must be vertically flush with zero gap and zero overlap, got gap=${gap}`);
}

async function assertChromeOffsetDriftGuard(page, origin) {
  await openPage(page, origin, appRoutes.root);
  const offsetPx = await chromeOffsetPx(page);
  const regionHeight = await page.locator("[data-ccresdoc-chrome-region]").evaluate((element) => element.getBoundingClientRect().height);
  assert(
    Math.abs(offsetPx - regionHeight) <= 1,
    `--ccresdoc-complete-chrome-offset (${offsetPx}px) must equal the chrome region's measured height (${regionHeight}px) — this is the drift guard that replaces a rejected ResizeObserver`,
  );
}

// The chrome region wrapper is deliberately NOT a zfb persist root (see the
// comment above ChromeRegion in pages/lib/_chrome.ts): it is expected to be
// a brand-new node after every soft navigation. Its two children each carry
// their own independent persist key and must keep node identity. The footer
// is a third, separate persist root and must also survive (issue #232's R3
// no-regression requirement).
async function assertChromeNodeIdentity(page, origin) {
  await openPage(page, origin, appRoutes.root);
  await page.evaluate(() => {
    window.__ccresdocChromeIdentity = {
      wrapper: document.querySelector("[data-ccresdoc-chrome-region]"),
      toolbarShell: document.querySelector("[data-ccresdoc-browser-toolbar-shell]"),
      header: document.querySelector("header[data-header]"),
      footer: document.querySelector("footer[data-zfb-transition-persist]"),
    };
  });
  await routeViaHeader(page, appRoutes.claude);
  const identity = await page.evaluate(() => ({
    wrapperReplaced: window.__ccresdocChromeIdentity.wrapper !== document.querySelector("[data-ccresdoc-chrome-region]"),
    toolbarPersisted: window.__ccresdocChromeIdentity.toolbarShell === document.querySelector("[data-ccresdoc-browser-toolbar-shell]"),
    headerPersisted: window.__ccresdocChromeIdentity.header === document.querySelector("header[data-header]"),
    footerPersisted: window.__ccresdocChromeIdentity.footer === document.querySelector("footer[data-zfb-transition-persist]"),
  }));
  assert.equal(identity.wrapperReplaced, true, "the non-persisted chrome region wrapper must be a new node after a soft navigation");
  assert.equal(identity.toolbarPersisted, true, "the browser toolbar shell must keep its own node identity across a soft navigation");
  assert.equal(identity.headerPersisted, true, "the package header must keep its own node identity across a soft navigation");
  assert.equal(identity.footerPersisted, true, "the footer must keep its existing persisted node identity across a soft navigation (no regression)");
}

async function assertChromeOffsetConsumers(page, origin) {
  await openPage(page, origin, appRoutes.root);
  const offsetPx = await chromeOffsetPx(page);

  const sidebar = page.locator("#desktop-sidebar");
  assert.equal(await sidebar.count(), 1, "desktop sidebar #desktop-sidebar must mount before its chrome offset can be checked");
  const sidebarTop = await sidebar.evaluate((element) => element.getBoundingClientRect().top);
  assert(sidebarTop >= offsetPx - 1, `desktop sidebar must start at/below the combined chrome height (${offsetPx}px), got top=${sidebarTop}`);

  const resizer = page.locator("[data-sidebar-resizer]");
  assert.equal(await resizer.count(), 1, "sidebar resizer handle [data-sidebar-resizer] must mount (settings.sidebarResizer is enabled)");
  const resizerTop = await resizer.evaluate((element) => element.getBoundingClientRect().top);
  assert(resizerTop >= offsetPx - 1, `sidebar resizer handle must start at/below the combined chrome height (${offsetPx}px), got top=${resizerTop}`);

  // No checked-in route renders real TOC headings — zudo-doc only mounts
  // its TOC island when `tocHeadings.length > 0`, and the checked-in
  // landing MDX files contain no prose headings. So this checks the
  // `nav[data-zd-toc]` CSS contract with a synthetic probe element carrying
  // that exact selector instead of a rendered TOC: selector-level coverage
  // only, not an exercise of the real TOC island.
  const tocProbeTop = await page.evaluate(() => {
    const probe = document.createElement("nav");
    probe.setAttribute("data-zd-toc", "");
    probe.style.cssText = "position:fixed;visibility:hidden;left:0";
    document.body.append(probe);
    const top = parseFloat(getComputedStyle(probe).top);
    probe.remove();
    return top;
  });
  assert(Math.abs(tocProbeTop - offsetPx) <= 1, `[data-zd-toc] must resolve its top offset to the combined chrome height (${offsetPx}px), got ${tocProbeTop}px`);

  await page.setViewportSize({ width: 390, height: 844 });
  await delay(50);
  const drawerToggle = page.locator('button[aria-label="Open sidebar"]').first();
  await drawerToggle.waitFor({ state: "visible", timeout: browserTimeoutMs });
  await drawerToggle.click();
  const drawer = page.locator("[data-zd-mobile-sidebar]");
  await page.waitForFunction(() => document.querySelector("[data-zd-mobile-sidebar]")?.hasAttribute("inert") === false, undefined, { timeout: browserTimeoutMs });
  const drawerOffsetPx = await chromeOffsetPx(page);
  const drawerTop = await drawer.evaluate((element) => element.getBoundingClientRect().top);
  assert(drawerTop >= drawerOffsetPx - 1, `mobile sidebar drawer must start at/below the combined chrome height (${drawerOffsetPx}px), got top=${drawerTop}`);
  await page.keyboard.press("Escape");
  await page.setViewportSize({ width: 1280, height: 900 });
}

async function assertOverflowMenuPaintsAboveHeader(page, origin) {
  await openPage(page, origin, appRoutes.root);
  await command(page, "more").click();
  const menuBox = await menu(page).boundingBox();
  const headerBox = await page.locator("header[data-header]").boundingBox();
  assert(menuBox && headerBox, "overflow menu and header must have geometry");
  const overlapTop = Math.max(menuBox.y, headerBox.y);
  const overlapBottom = Math.min(menuBox.y + menuBox.height, headerBox.y + headerBox.height);
  assert(overlapBottom > overlapTop, `overflow menu must visually overlap the header row to exercise the stacking order: menu=${JSON.stringify(menuBox)}, header=${JSON.stringify(headerBox)}`);
  const point = { x: menuBox.x + menuBox.width / 2, y: (overlapTop + overlapBottom) / 2 };
  const hitInsideMenu = await page.evaluate(({ x, y }) => {
    const hit = document.elementFromPoint(x, y);
    const menuElement = document.querySelector('nav.ccresdoc-browser-toolbar [role="menu"]');
    return Boolean(hit && menuElement && menuElement.contains(hit));
  }, point);
  assert(hitInsideMenu, `the overflow menu must paint above the header at the overlapping point ${JSON.stringify(point)}`);
  await page.keyboard.press("Escape");
}

async function assertThemeArtifacts(page, origin) {
  if (!artifactDir) return;
  await openPage(page, origin, appRoutes.root);
  await page.setViewportSize({ width: 1280, height: 900 });
  const toDark = page.locator('[data-zfb-island="ThemeToggle"] button[aria-label^="Switch to dark"]').first();
  await toDark.waitFor({ state: "visible", timeout: browserTimeoutMs });
  await toDark.click();
  await page.waitForFunction(() => document.documentElement.getAttribute("data-theme") === "dark", undefined, { timeout: browserTimeoutMs });
  await page.screenshot({ path: join(artifactDir, "browser-toolbar-desktop-dark.png") });
  await page.setViewportSize({ width: 390, height: 844 });
  await delay(50);
  await page.screenshot({ path: join(artifactDir, "browser-toolbar-narrow-dark.png") });
  await page.setViewportSize({ width: 1280, height: 900 });
  const toLight = page.locator('[data-zfb-island="ThemeToggle"] button[aria-label^="Switch to light"]').first();
  await toLight.waitFor({ state: "visible", timeout: browserTimeoutMs });
  await toLight.click();
  await page.waitForFunction(() => document.documentElement.getAttribute("data-theme") === "light", undefined, { timeout: browserTimeoutMs });
}

async function assertCoarseTargets(browser, origin) {
  const context = await browser.newContext({
    viewport: { width: 390, height: 844 },
    hasTouch: true,
    isMobile: true,
  });
  await installTauriHarness(context);
  const page = await context.newPage();
  try {
    await openPage(page, origin, appRoutes.root);
    const metrics = await geometry(page);
    assert.equal(metrics.mediaCoarse, true, "coarse-pointer context must expose pointer: coarse media");
    assert(metrics.button.width >= 44 && metrics.button.height >= 44, `coarse toolbar target must be at least 44px, got ${JSON.stringify(metrics.button)}`);
    await command(page, "more").click();
    const row = await menu(page).locator('[role="menuitem"]').first().boundingBox();
    assert(row && row.width >= 44 && row.height >= 44, `coarse More row must be at least 44px, got ${JSON.stringify(row)}`);
  } finally {
    await context.close();
  }
}

async function assertBrowserOnly(browser, origin) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin });
  const pageErrors = [];
  const page = await context.newPage();
  page.on("pageerror", (error) => pageErrors.push(error.message));
  try {
    await openPage(page, origin, appRoutes.root);
    assert.equal(await page.evaluate(() => "__TAURI__" in window), false, "ordinary browser mode must not install a Tauri bridge");
    assert.equal(await command(page, "back").isDisabled(), true, "browser-only Back starts at the managed boundary");
    assert.equal(await command(page, "forward").isDisabled(), true, "browser-only Forward starts at the managed boundary");
    assert.equal(await command(page, "reload-documentation").isDisabled(), true, "browser-only Reload is unavailable");
    assert.equal(await command(page, "open-in-default-browser").isDisabled(), true, "browser-only external-open is unavailable");
    await command(page, "more").click();
    assert.equal(await menu(page).locator('[data-browser-command="settings"]').isDisabled(), true, "browser-only Settings is unavailable");
    assert.equal(await menu(page).locator('[data-browser-command="open-in-default-browser"]').isDisabled(), true, "browser-only More external-open is unavailable");
    await page.keyboard.press("Escape");

    await command(page, "copy-page-path").click();
    await assertClipboard(page, appRoutes.root, "browser-only Copy remains functional");
    await openControlledSearch(page, true);
    await closeSearch(page);

    let promptSeen = false;
    const prompt = page.waitForEvent("dialog");
    const findClick = command(page, "find-in-page").click();
    const dialog = await prompt;
    promptSeen = dialog.type() === "prompt";
    await dialog.accept("CCResDoc");
    await findClick;
    assert.equal(promptSeen, true, "browser-only toolbar Find delegates to the ordinary browser Find prompt");
    await page.keyboard.press("Control+F");
    await delay(100);
    assert.equal(await page.locator("[data-find-in-page-bar]").count(), 0, "browser-only Control+F does not install the privileged Find bar");
    await command(page, "home").click();
    await waitForPath(page, appRoutes.root);
    await routeViaHeader(page, appRoutes.claude);
    await waitForCommandEnabled(page, "back");
    await page.keyboard.press("Control+[");
    await waitForPath(page, appRoutes.root);
    await waitForCommandEnabled(page, "forward");
    await page.keyboard.press("Control+]");
    await waitForPath(page, appRoutes.claude);
    await command(page, "home").click();
    await waitForPath(page, appRoutes.root);
    const missing = await fetchText(`${origin}${appRoutes.missing}`);
    assert.equal(missing.status, 404, "the deterministic missing route keeps its HTTP 404 contract");
    // zfb dev intentionally answers an unknown URL with its own minimal 404.
    // Open the generated host page to exercise CCResDoc's toolbar and Home
    // recovery behavior instead of asserting against that carrier fallback.
    await openPage(page, origin, appRoutes.notFound);
    assert.match(await page.locator("body").innerText(), /Page not found/i, "the checked-in host 404 renders through package chrome");
    await command(page, "home").click();
    await waitForPath(page, appRoutes.root);
    assert.equal(await page.evaluate(() => "__TAURI__" in window), false, "browser-only interactions never create a Tauri bridge");
    assert.deepEqual(pageErrors, [], `browser-only interactions raised page exceptions: ${pageErrors.join("; ")}`);
  } finally {
    await context.close();
  }
}

async function run() {
  const options = parseOptions();
  assertRepositoryContracts();
  if (options.contracts) {
    console.log("browser-navigation repository/runtime contracts passed");
    return;
  }
  if (artifactDir) mkdirSync(artifactDir, { recursive: true });
  const removeSignalCleanup = installSignalCleanup();
  try {
    const server = await startServer();
    try {
      let browser;
      try {
        const playwright = appRequire("playwright");
        browser = await playwright.chromium.launch({ headless: !options.headed });
        activeBrowser = browser;
      } catch (error) {
        throw new Error(`Chromium is not installed; run 'pnpm --dir app exec playwright install chromium': ${error.message}`, { cause: error });
      }

      const context = await browser.newContext({
        viewport: { width: 1280, height: 900 },
        permissions: ["clipboard-read", "clipboard-write"],
      });
      await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin: server.origin });
      await installTauriHarness(context);
      const page = await context.newPage();
      try {
        await assertHistorySurface(page, server.origin);
        await assertReloadAndPageshow(page, server.origin);
        await assertFindSurface(page);
        await assertSearchSurface(page);
        await assertEditingTargetSuppression(page);
        await assertShortcutReconfiguration(page);
        await assertMoreAndToolbarActions(page);
        await assertResponsiveGeometry(page, server.origin);
        await assertStickyChromeRegion(page, server.origin);
        await assertChromeAdjacency(page, server.origin);
        await assertChromeOffsetDriftGuard(page, server.origin);
        await assertChromeNodeIdentity(page, server.origin);
        await assertChromeOffsetConsumers(page, server.origin);
        await assertOverflowMenuPaintsAboveHeader(page, server.origin);
        await assertThemeArtifacts(page, server.origin);
      } finally {
        await context.close();
      }
      await assertCoarseTargets(browser, server.origin);
      await assertBrowserOnly(browser, server.origin);
      console.log(JSON.stringify({
        status: "passed",
        browser: "chromium",
        actualKeyboardInput: "page.keyboard.press",
        origin: server.origin,
        routes: appRoutes,
        semanticModules: server.modules.length,
        browserOnly: "no Tauri bridge; host-only actions unavailable",
        macosNativeArbitration: "manager/release-machine gate required",
      }, null, 2));
    } finally {
      await closeActiveResources();
    }
  } finally {
    removeSignalCleanup();
  }
}

run().catch((error) => {
  console.error(error?.stack ?? error);
  process.exitCode = 1;
});
