#!/usr/bin/env node

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import catalog from "@takazudo/zudo-doc/catalog";

export const catalogFile = fileURLToPath(import.meta.resolve("@takazudo/zudo-doc/catalog"));
export const packageThemePacksRoot = join(dirname(catalogFile), "theme-packs");
export const generatedThemePacksRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../public/theme-packs");

const slugPattern = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
const versionPattern = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/;
const cssUrlPattern = /url\(\s*(["']?)([^"')]+)\1\s*\)/g;
const fontFacePattern = /@font-face\s*{([\s\S]*?)}/g;
const fontFamilyPattern = /font-family:\s*(["'])(.*?)\1\s*;/;

function fail(message) {
  throw new Error(`theme-pack sync: ${message}`);
}

function json(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

function assertFile(path, label) {
  if (!existsSync(path) || !statSync(path).isFile()) fail(`${label} is missing: ${path}`);
}

function assertSameBytes(actualPath, expectedPath, label) {
  assertFile(actualPath, label);
  if (!readFileSync(actualPath).equals(readFileSync(expectedPath))) {
    fail(`${label} differs from the package asset: ${actualPath}`);
  }
}

function validateMetadata(meta) {
  if (typeof meta.name !== "string" || meta.name.length === 0 || typeof meta.description !== "string") {
    fail(`pack "${meta.slug}" has invalid name or description metadata`);
  }
  if (meta.mode !== "light" && meta.mode !== "dark") fail(`pack "${meta.slug}" has an invalid mode`);
  if (!meta.fonts || typeof meta.fonts.sans !== "string" || typeof meta.fonts.mono !== "string"
    || !Array.isArray(meta.fonts.loaded) || meta.fonts.loaded.some((name) => typeof name !== "string" || name.length === 0)) {
    fail(`pack "${meta.slug}" has invalid font metadata`);
  }
  if (meta.fonts.display !== undefined && typeof meta.fonts.display !== "string") {
    fail(`pack "${meta.slug}" has invalid display-font metadata`);
  }
  for (const mode of ["light", "dark"]) {
    const preview = meta.preview?.[mode];
    if (!preview || [preview.bg, preview.fg, preview.accent].some((value) => typeof value !== "string" || value.length === 0)) {
      fail(`pack "${meta.slug}" has invalid ${mode} preview metadata`);
    }
    if (!preview.syntax || ["keyword", "string", "comment", "callable"]
      .some((key) => typeof preview.syntax[key] !== "string" || preview.syntax[key].length === 0)) {
      fail(`pack "${meta.slug}" has invalid ${mode} syntax preview metadata`);
    }
  }
}

function walkFiles(root, current = root) {
  if (!existsSync(root)) return [];
  const files = [];
  for (const entry of readdirSync(current, { withFileTypes: true })) {
    const path = join(current, entry.name);
    if (entry.isDirectory()) files.push(...walkFiles(root, path));
    else if (entry.isFile()) files.push(relative(root, path).split(sep).join("/"));
    else fail(`generated output contains a non-file entry: ${path}`);
  }
  return files.sort();
}

function fontPaths(css, slug) {
  const paths = new Set();
  for (const match of css.matchAll(cssUrlPattern)) {
    const url = match[2].trim();
    if (url.startsWith("data:") || url.startsWith("#")) continue;
    if (!url.startsWith("./fonts/") || url.includes("?") || url.includes("#")) {
      fail(`pack "${slug}" has an unsupported asset URL in pack.css: ${url}`);
    }
    const path = url.slice(2);
    if (path.split("/").includes("..")) fail(`pack "${slug}" has a traversing font URL: ${url}`);
    paths.add(path);
  }
  return [...paths].sort();
}

export function inspectThemePackInputs({ manifest = catalog, sourceRoot = packageThemePacksRoot } = {}) {
  if (manifest?.schemaVersion !== 1 || !Array.isArray(manifest.packs)) {
    fail("catalog must have schemaVersion 1 and a packs array");
  }
  if (manifest.packs.length === 0) fail("catalog must contain at least the default pack");

  const slugs = new Set();
  const packs = [];
  for (const meta of manifest.packs) {
    if (meta?.schemaVersion !== 1) fail(`pack metadata has an unsupported schemaVersion: ${meta?.slug ?? "(unknown)"}`);
    if (!slugPattern.test(meta.slug ?? "")) fail(`catalog contains an invalid slug: ${JSON.stringify(meta?.slug)}`);
    if (slugs.has(meta.slug)) fail(`catalog contains duplicate slug: ${meta.slug}`);
    if (!versionPattern.test(meta.version ?? "")) fail(`pack "${meta.slug}" has an invalid version: ${JSON.stringify(meta.version)}`);
    validateMetadata(meta);
    slugs.add(meta.slug);

    const packRoot = join(sourceRoot, meta.slug);
    const metaPath = join(packRoot, "meta.json");
    assertFile(metaPath, `pack "${meta.slug}" metadata`);
    let packageMeta;
    try {
      packageMeta = JSON.parse(readFileSync(metaPath, "utf8"));
    } catch (error) {
      fail(`pack "${meta.slug}" metadata is not valid JSON: ${error.message}`);
    }
    try {
      assert.deepStrictEqual(packageMeta, meta);
    } catch {
      fail(`pack "${meta.slug}" metadata differs from the public catalog`);
    }

    const cssPath = join(packRoot, "pack.css");
    if (meta.slug === "default") {
      if (existsSync(cssPath)) fail('reserved pack "default" must be metadata-only (pack.css is forbidden)');
      packs.push({ meta, metaPath, cssPath: null, fonts: [] });
      continue;
    }

    assertFile(cssPath, `pack "${meta.slug}" stylesheet`);
    const css = readFileSync(cssPath, "utf8");
    if (css.trim().length === 0) fail(`pack "${meta.slug}" stylesheet is empty`);
    const declaredFontFamilies = new Set([...css.matchAll(fontFacePattern)]
      .map((match) => fontFamilyPattern.exec(match[1])?.[2])
      .filter(Boolean));
    const missingFamilies = meta.fonts.loaded.filter((family) => !declaredFontFamilies.has(family));
    if (missingFamilies.length > 0) {
      fail(`pack "${meta.slug}" fonts.loaded has no matching @font-face: ${missingFamilies.join(", ")}`);
    }
    const fonts = fontPaths(css, meta.slug).map((path) => {
      const source = join(packRoot, path);
      assertFile(source, `font referenced by pack "${meta.slug}"`);
      return { path, source };
    });
    packs.push({ meta, metaPath, cssPath, fonts });
  }
  if (!slugs.has("default")) fail('catalog is missing the reserved "default" pack');
  return packs;
}

export function themePackInputFiles(options = {}) {
  const inputs = inspectThemePackInputs(options);
  const files = options.manifest === undefined ? [catalogFile] : [];
  for (const pack of inputs) {
    files.push(pack.metaPath);
    if (pack.cssPath) files.push(pack.cssPath);
    files.push(...pack.fonts.map(({ source }) => source));
  }
  return [...new Set(files)].sort();
}

export function themePackInputDigest(options = {}) {
  const manifest = options.manifest ?? catalog;
  const sourceRoot = options.sourceRoot ?? packageThemePacksRoot;
  const implementationPath = options.implementationPath ?? fileURLToPath(import.meta.url);
  const catalogSourcePath = Object.hasOwn(options, "catalogSourcePath")
    ? options.catalogSourcePath
    : manifest === catalog ? catalogFile : null;
  const hash = createHash("sha256");
  hash.update("sync-theme-packs.mjs\0").update(readFileSync(implementationPath)).update("\0");
  if (catalogSourcePath) {
    hash.update("catalog.js\0").update(readFileSync(catalogSourcePath)).update("\0");
  } else {
    hash.update("catalog.json\0").update(json(manifest)).update("\0");
  }
  for (const input of themePackInputFiles({ manifest, sourceRoot })) {
    const label = `theme-packs/${relative(sourceRoot, input).split(sep).join("/")}`;
    hash.update(label).update("\0").update(readFileSync(input)).update("\0");
  }
  return hash.digest("hex");
}

export function validateGeneratedThemePacks({
  manifest = catalog,
  sourceRoot = packageThemePacksRoot,
  outputRoot = generatedThemePacksRoot,
} = {}) {
  const packs = inspectThemePackInputs({ manifest, sourceRoot });
  const indexPath = join(outputRoot, "index.json");
  assertFile(indexPath, "generated catalog");
  if (readFileSync(indexPath, "utf8") !== json(manifest)) {
    fail(`generated catalog differs from @takazudo/zudo-doc/catalog: ${indexPath}`);
  }

  const expected = new Set(["index.json"]);
  for (const pack of packs) {
    if (!pack.cssPath) continue;
    const cssRelative = `${pack.meta.slug}/pack.css`;
    expected.add(cssRelative);
    assertSameBytes(join(outputRoot, cssRelative), pack.cssPath, `generated stylesheet for "${pack.meta.slug}"`);
    for (const font of pack.fonts) {
      const fontRelative = `${pack.meta.slug}/${font.path}`;
      expected.add(fontRelative);
      assertSameBytes(join(outputRoot, fontRelative), font.source, `generated font for "${pack.meta.slug}"`);
    }
  }

  const actual = walkFiles(outputRoot);
  const missing = [...expected].filter((path) => !actual.includes(path));
  const extra = actual.filter((path) => !expected.has(path));
  if (missing.length > 0) fail(`generated output is missing: ${missing.join(", ")}`);
  if (extra.length > 0) fail(`generated output contains stale or extra files: ${extra.join(", ")}`);
  return { packs: packs.length, files: actual.length };
}

export function syncThemePacks({
  manifest = catalog,
  sourceRoot = packageThemePacksRoot,
  outputRoot = generatedThemePacksRoot,
} = {}) {
  const packs = inspectThemePackInputs({ manifest, sourceRoot });
  rmSync(outputRoot, { recursive: true, force: true });
  mkdirSync(outputRoot, { recursive: true });
  writeFileSync(join(outputRoot, "index.json"), json(manifest));

  for (const pack of packs) {
    if (!pack.cssPath) continue;
    const destination = join(outputRoot, pack.meta.slug);
    mkdirSync(destination, { recursive: true });
    copyFileSync(pack.cssPath, join(destination, "pack.css"));
    for (const font of pack.fonts) {
      const fontDestination = join(destination, font.path);
      mkdirSync(dirname(fontDestination), { recursive: true });
      copyFileSync(font.source, fontDestination);
    }
  }
  return validateGeneratedThemePacks({ manifest, sourceRoot, outputRoot });
}

const isMain = process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href;
if (isMain) {
  try {
    const result = process.argv.includes("--check")
      ? validateGeneratedThemePacks()
      : syncThemePacks();
    console.log(`theme-pack assets ${process.argv.includes("--check") ? "validated" : "synced"}: ${result.packs} packs, ${result.files} files`);
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
