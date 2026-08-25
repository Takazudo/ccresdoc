#!/usr/bin/env node

/*
 * Browser confirmation for resource-search issue #199.
 *
 * Run against an already-running dev server:
 *
 *   pnpm --dir app exec playwright install chromium  # once, if needed
 *   pnpm dev
 *   pnpm run test:search-shortcuts
 *
 * The FindInPageInit island intentionally self-gates on the Tauri runtime
 * marker.  This harness installs the marker before any page script executes;
 * without that init script, a plain browser would render no find bar and the
 * Cmd/Ctrl+F checks would be false passes.
 *
 * Use --find-mount-only for a small regression proof.  Temporarily remove the
 * FindInPageInit island from app/pages/lib/_chrome.ts, run this mode, and the
 * command must fail before reporting a green result.
 */

import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const appRoot = join(repoRoot, "app");
const appRequire = createRequire(join(appRoot, "package.json"));
const defaultUrl = "http://127.0.0.1:4892/docs/";
const timeoutMs = 10_000;
const searchTerm = "resource";
const modifiers = [
  { name: "Meta", label: "Cmd", metaKey: true, ctrlKey: false },
  { name: "Control", label: "Ctrl", metaKey: false, ctrlKey: true },
];

function usage() {
  console.log(`Usage:
  pnpm --dir app exec playwright install chromium  # once, if needed
  pnpm dev
  pnpm run test:search-shortcuts

Options:
  --url=<url>          Docs URL (default: ${defaultUrl})
  --headed             Show Chromium instead of running headless
  --find-mount-only    Check only the Cmd/Ctrl+F mount and gate
  --help               Show this help

Regression proof:
  Temporarily remove the FindInPageInit island from app/pages/lib/_chrome.ts,
  then run \`pnpm run test:search-shortcuts -- --find-mount-only\`.  The command
  must exit non-zero with a missing-island or missing-find-bar assertion.
`);
}

function parseOptions() {
  const args = process.argv.slice(2);
  if (args.includes("--help") || args.includes("-h")) {
    usage();
    process.exit(0);
  }
  const urlArg = args.find((arg) => arg.startsWith("--url="));
  const url = urlArg ? urlArg.slice("--url=".length) : process.env.CCRESDOC_BROWSER_URL ?? defaultUrl;
  const browserUrl = new URL(url);
  browserUrl.pathname = browserUrl.pathname.endsWith("/") ? browserUrl.pathname : `${browserUrl.pathname}/`;
  return {
    url: browserUrl.toString(),
    headed: args.includes("--headed"),
    findMountOnly: args.includes("--find-mount-only"),
  };
}

function assertPinnedPlaywright() {
  const packageJson = JSON.parse(readFileSync(join(appRoot, "package.json"), "utf8"));
  assert.equal(
    packageJson.devDependencies?.playwright,
    "1.62.1",
    "app/package.json must keep the pinned Playwright dependency",
  );
  assert.match(
    readFileSync(join(appRoot, "pnpm-lock.yaml"), "utf8"),
    /playwright(?:-core)?@1\.62\.1/,
    "app/pnpm-lock.yaml must keep the pinned Playwright resolution",
  );
}

async function waitForTwoFrames(page) {
  await page.evaluate(() => new Promise((resolveFrame) => {
    requestAnimationFrame(() => requestAnimationFrame(resolveFrame));
  }));
}

async function waitForFindBar(page, visible) {
  await page.waitForFunction(
    (expected) => Boolean(document.querySelector('input[aria-label="Find in page"]')) === expected,
    visible,
    { timeout: timeoutMs },
  );
}

async function waitForSearchDialog(page, open) {
  await page.waitForFunction(
    (expected) => Boolean(document.querySelector("dialog[data-search-dialog]")?.open) === expected,
    open,
    { timeout: timeoutMs },
  );
}

async function waitForMarks(page, minimum) {
  await page.waitForFunction(
    (expected) => document.querySelectorAll("mark.find-match").length >= expected,
    minimum,
    { timeout: timeoutMs },
  );
}

async function dispatchShortcut(page, key, modifier) {
  await page.evaluate(({ key: eventKey, modifierName, metaKey, ctrlKey }) => {
    const event = new KeyboardEvent("keydown", {
      key: eventKey,
      bubbles: true,
      cancelable: true,
      metaKey,
      ctrlKey,
    });
    // Dispatch from the focused control so surface-aware implementations see
    // the same target as a user shortcut inside the open dialog/find input.
    const target = document.activeElement instanceof HTMLElement ? document.activeElement : document;
    target.dispatchEvent(event);
    window.__ccresdocLastShortcut = `${modifierName}+${eventKey}`;
  }, {
    key,
    modifierName: modifier.name,
    metaKey: modifier.metaKey,
    ctrlKey: modifier.ctrlKey,
  });
  // The find island adds its document listener from a Preact effect.  Two
  // frames keep this helper deterministic without a timing-based sleep.
  await waitForTwoFrames(page);
}

async function assertFindMount(page) {
  const initMarker = page.locator('[data-zfb-island="FindInPageInit"]');
  assert.equal(
    await initMarker.count(),
    1,
    "FindInPageInit island marker must be present; Cmd/Ctrl+F coverage cannot pass by testing nothing",
  );
  const gate = await page.evaluate(() => ({
    tauriStubPresent: "__TAURI_INTERNALS__" in window,
    initScriptRan: window.__ccresdocTauriInitScript === true,
  }));
  assert.equal(gate.tauriStubPresent, true, "the pre-script __TAURI_INTERNALS__ stub is missing");
  assert.equal(gate.initScriptRan, true, "the Playwright pre-script init marker did not run");
}

async function activeMatchIndex(page) {
  return page.evaluate(() => {
    const marks = [...document.querySelectorAll("mark.find-match")];
    return marks.findIndex((mark) => mark.classList.contains("find-match-active"));
  });
}

async function assertMatchScope(page) {
  const scope = await page.evaluate(() => {
    const marks = [...document.querySelectorAll("mark.find-match")];
    const article = document.querySelector("article.zd-content");
    const inArticle = marks.filter((mark) => mark.closest("article.zd-content") === article).length;
    return {
      total: marks.length,
      inArticle,
      outsideArticle: marks.filter((mark) => !mark.closest("article.zd-content")).length,
      inHeader: document.querySelectorAll("header mark.find-match").length,
      inSidebar: document.querySelectorAll("aside mark.find-match").length,
      inSearchDialog: document.querySelectorAll("[data-search-dialog] mark.find-match").length,
      active: document.querySelectorAll("mark.find-match-active").length,
    };
  });
  assert(scope.total >= 2, `expected at least two ${searchTerm} matches, got ${scope.total}`);
  assert.equal(scope.inArticle, scope.total, "find marks must be confined to article.zd-content");
  assert.equal(scope.outsideArticle, 0, "find marks leaked outside article.zd-content");
  assert.equal(scope.inHeader, 0, "find marks leaked into the header");
  assert.equal(scope.inSidebar, 0, "find marks leaked into the sidebar");
  assert.equal(scope.inSearchDialog, 0, "find marks leaked into the search dialog");
  assert.equal(scope.active, 1, "exactly one find match must be active");
}

async function openFindBar(page, modifier) {
  await dispatchShortcut(page, "f", modifier);
  await waitForFindBar(page, true);
  assert.equal(
    await page.locator('input[aria-label="Find in page"]').count(),
    1,
    `${modifier.label}+F must render one find input after the Tauri gate is stubbed`,
  );
  assert.equal(
    await page.locator("dialog[data-search-dialog]").evaluate((dialog) => dialog.open),
    false,
    `${modifier.label}+F must not open the search dialog`,
  );
}

async function closeFindBar(page) {
  await page.keyboard.press("Escape");
  await waitForFindBar(page, false);
  await page.waitForFunction(
    () => document.querySelectorAll("mark.find-match").length === 0,
    undefined,
    { timeout: timeoutMs },
  );
  assert.equal(
    await page.locator("dialog[data-search-dialog]").evaluate((dialog) => dialog.open),
    false,
    "Escape must not open or leave the search dialog when only the find bar was open",
  );
}

async function closeSearchDialog(page) {
  await page.keyboard.press("Escape");
  await waitForSearchDialog(page, false);
  await waitForFindBar(page, false);
  assert.equal(
    await page.locator("mark.find-match").count(),
    0,
    "Escape must not create find marks when only the search dialog was open",
  );
}

async function assertSpaCleanup(page, modifier) {
  await openFindBar(page, modifier);
  await page.locator('input[aria-label="Find in page"]').fill(searchTerm);
  await waitForMarks(page, 2);

  const navLink = page.locator(
    'header a[href*="/docs/claude"], header a[href*="/docs/codex"]',
  ).first();
  await navLink.waitFor({ state: "visible", timeout: timeoutMs });
  await page.evaluate(() => {
    window.__ccresdocBeforePreparationSeen = false;
    document.addEventListener(
      "zfb:before-preparation",
      () => { window.__ccresdocBeforePreparationSeen = true; },
      { once: true },
    );
  });
  await navLink.click();
  await page.waitForFunction(
    () => window.__ccresdocBeforePreparationSeen === true,
    undefined,
    { timeout: timeoutMs },
  );
  await page.waitForFunction(
    () => document.querySelectorAll("mark.find-match").length === 0,
    undefined,
    { timeout: timeoutMs },
  );
  await waitForFindBar(page, false);
}

async function preparePage(page, url) {
  await page.goto(url, { waitUntil: "domcontentloaded", timeout: 30_000 });
  await page.waitForFunction(() => document.readyState !== "loading", undefined, { timeout: timeoutMs });
  await assertFindMount(page);
  await waitForTwoFrames(page);
  await waitForSearchDialog(page, false);
  await waitForFindBar(page, false);
}

async function runFindMountProbe(page, url, modifier) {
  await preparePage(page, url);
  await openFindBar(page, modifier);
  await closeFindBar(page);
}

async function runModifierScenario(page, url, modifier) {
  await preparePage(page, url);

  // Cmd/Ctrl+K opens search, while Cmd/Ctrl+F leaves the find surface closed.
  await dispatchShortcut(page, "k", modifier);
  await waitForSearchDialog(page, true);
  assert.equal(
    await page.locator('input[aria-label="Find in page"]').count(),
    0,
    `${modifier.label}+K must not open the find bar`,
  );
  assert.equal(
    await page.locator("[data-search-dialog] mark.find-match").count(),
    0,
    "the open search dialog must contain no find marks",
  );

  await dispatchShortcut(page, "f", modifier);
  await waitForSearchDialog(page, true);
  await waitForFindBar(page, false);
  await closeSearchDialog(page);

  // Cmd/Ctrl+F opens find, and Cmd/Ctrl+K must leave it as the only surface.
  await openFindBar(page, modifier);
  const findInput = page.locator('input[aria-label="Find in page"]');
  await findInput.fill(searchTerm);
  await waitForMarks(page, 2);
  await assertMatchScope(page);

  await dispatchShortcut(page, "k", modifier);
  await waitForFindBar(page, true);
  await waitForSearchDialog(page, false);

  const initialIndex = await activeMatchIndex(page);
  assert.equal(initialIndex, 0, `${modifier.label}+F must activate the first find match`);
  // Exercise FindBar's supported input keyboard path so Escape below remains
  // focused on the same input (the shipped component handles Escape there).
  await findInput.press("Enter");
  const nextIndex = await activeMatchIndex(page);
  assert.notEqual(nextIndex, initialIndex, `${modifier.label}+F Next must move the active match`);
  await findInput.press("Shift+Enter");
  assert.equal(
    await activeMatchIndex(page),
    initialIndex,
    `${modifier.label}+F Previous must return to the prior active match`,
  );
  await closeFindBar(page);

  // A fresh mark set must be cleared by an actual SPA before-preparation hook.
  await assertSpaCleanup(page, modifier);
  assert.equal(
    await page.locator("dialog[data-search-dialog]").evaluate((dialog) => dialog.open),
    false,
    "the before-preparation cleanup must not leave the search dialog open",
  );
}

async function main() {
  const options = parseOptions();
  assertPinnedPlaywright();
  const { chromium } = appRequire("playwright");
  let browser;
  try {
    browser = await chromium.launch({ headless: !options.headed });
  } catch (error) {
    throw new Error(
      `Chromium is not installed; run 'pnpm --dir app exec playwright install chromium': ${error.message}`,
      { cause: error },
    );
  }
  const context = await browser.newContext();
  // FindInPageInit intentionally checks this marker before installing its
  // listener. addInitScript is the only reliable way to emulate the Tauri
  // WebView without allowing page code to race the gate.
  await context.addInitScript(() => {
    if (!("__TAURI_INTERNALS__" in window)) window.__TAURI_INTERNALS__ = {};
    window.__ccresdocTauriInitScript = true;
  });
  const page = await context.newPage();
  try {
    if (options.findMountOnly) {
      for (const modifier of modifiers) {
        console.log(`Checking ${modifier.label}+F mount...`);
        await runFindMountProbe(page, options.url, modifier);
      }
      console.log(JSON.stringify({ status: "passed", mode: "find-mount-only", modifiers: modifiers.map(({ label }) => label) }, null, 2));
      return;
    }

    for (const modifier of modifiers) {
      console.log(`Checking ${modifier.label}/Ctrl coexistence...`);
      await runModifierScenario(page, options.url, modifier);
    }
    console.log(JSON.stringify({
      status: "passed",
      url: options.url,
      modifiers: modifiers.map(({ label }) => label),
      searchTerm,
      tauriInitScript: "window.__TAURI_INTERNALS__ = {}",
      regressionProbe: "pnpm run test:search-shortcuts -- --find-mount-only",
    }, null, 2));
  } finally {
    await context.close();
    await browser.close();
  }
}

main().catch((error) => {
  console.error(error?.stack ?? error);
  process.exitCode = 1;
});
