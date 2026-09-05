import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { afterEach, test } from "node:test";
import { syncThemePacks, themePackInputDigest, validateGeneratedThemePacks } from "./sync-theme-packs.mjs";

const roots = [];
afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "ccresdoc-theme-packs-"));
  roots.push(root);
  const sourceRoot = join(root, "source");
  const outputRoot = join(root, "output");
  const preview = {
    light: { bg: "#fff", fg: "#000", accent: "#00f", syntax: { keyword: "#00f", string: "#080", comment: "#555", callable: "#008" } },
    dark: { bg: "#000", fg: "#fff", accent: "#88f", syntax: { keyword: "#88f", string: "#8f8", comment: "#aaa", callable: "#8ff" } },
  };
  const defaultMeta = {
    schemaVersion: 1,
    slug: "default",
    name: "Default",
    description: "No stylesheet.",
    mode: "dark",
    version: "1.0.0",
    fonts: { sans: "System", mono: "System", loaded: [] },
    preview,
  };
  const inkMeta = {
    schemaVersion: 1,
    slug: "ink",
    name: "Ink",
    description: "Fixture pack.",
    mode: "light",
    version: "2.3.4",
    fonts: { sans: "Fixture", mono: "System", loaded: ["Fixture"] },
    preview,
  };
  const manifest = {
    schemaVersion: 2,
    packs: [
      { slug: "default", meta: defaultMeta, hasStylesheet: false },
      { slug: "ink", meta: inkMeta, hasStylesheet: true },
    ],
  };
  mkdirSync(join(sourceRoot, "default"), { recursive: true });
  mkdirSync(join(sourceRoot, "ink", "fonts"), { recursive: true });
  writeFileSync(join(sourceRoot, "default", "meta.json"), JSON.stringify(defaultMeta));
  writeFileSync(join(sourceRoot, "ink", "meta.json"), JSON.stringify(inkMeta));
  writeFileSync(join(sourceRoot, "ink", "pack.css"), '@font-face { font-family: "Fixture"; src: url("./fonts/fixture.woff2") format("woff2"); }\n');
  writeFileSync(join(sourceRoot, "ink", "fonts", "fixture.woff2"), Buffer.from([0, 1, 2, 255]));
  return { manifest, sourceRoot, outputRoot };
}

test("sync writes an exact catalog-derived tree and is idempotent", () => {
  const options = fixture();
  syncThemePacks(options);
  const firstIndex = readFileSync(join(options.outputRoot, "index.json"));
  const firstCss = readFileSync(join(options.outputRoot, "ink", "pack.css"));
  const firstFont = readFileSync(join(options.outputRoot, "ink", "fonts", "fixture.woff2"));

  writeFileSync(join(options.outputRoot, "stale.css"), "stale");
  syncThemePacks(options);

  assert.deepEqual(readFileSync(join(options.outputRoot, "index.json")), firstIndex);
  assert.deepEqual(readFileSync(join(options.outputRoot, "ink", "pack.css")), firstCss);
  assert.deepEqual(readFileSync(join(options.outputRoot, "ink", "fonts", "fixture.woff2")), firstFont);
  assert.deepEqual(validateGeneratedThemePacks(options), { packs: 2, files: 3 });
});

test("validation reports missing, stale, and byte-mismatched generated assets", () => {
  const options = fixture();
  syncThemePacks(options);

  writeFileSync(join(options.outputRoot, "ink", "pack.css"), "changed");
  assert.throws(() => validateGeneratedThemePacks(options), /differs from the package asset/);

  syncThemePacks(options);
  writeFileSync(join(options.outputRoot, "extra.txt"), "extra");
  assert.throws(() => validateGeneratedThemePacks(options), /stale or extra files/);

  syncThemePacks(options);
  rmSync(join(options.outputRoot, "ink", "fonts", "fixture.woff2"));
  assert.throws(() => validateGeneratedThemePacks(options), /is missing/);
});

test("source validation rejects catalog drift and missing referenced fonts", () => {
  const options = fixture();
  const drifted = structuredClone(options.manifest);
  drifted.packs[1].meta.version = "9.9.9";
  assert.throws(() => syncThemePacks({ ...options, manifest: drifted }), /metadata differs from the public catalog/);

  rmSync(join(options.sourceRoot, "ink", "fonts", "fixture.woff2"));
  assert.throws(() => syncThemePacks(options), /font referenced by pack "ink" is missing/);
});

test("default stays metadata-only and non-default packs require CSS", () => {
  const options = fixture();
  writeFileSync(join(options.sourceRoot, "default", "pack.css"), "body {}\n");
  assert.throws(() => syncThemePacks(options), /default.*metadata-only/);

  rmSync(join(options.sourceRoot, "default", "pack.css"));
  rmSync(join(options.sourceRoot, "ink", "pack.css"));
  assert.throws(() => syncThemePacks(options), /stylesheet is missing/);
});

test("catalog v2 entry fields agree with the package metadata and assets", () => {
  const options = fixture();
  const mismatchedSlug = structuredClone(options.manifest);
  mismatchedSlug.packs[1].slug = "other";
  assert.throws(() => syncThemePacks({ ...options, manifest: mismatchedSlug }), /entry slug differs/);

  const missingStylesheetFlag = structuredClone(options.manifest);
  missingStylesheetFlag.packs[1].hasStylesheet = false;
  assert.throws(() => syncThemePacks({ ...options, manifest: missingStylesheetFlag }), /must declare hasStylesheet: true/);

  const legacyCatalog = structuredClone(options.manifest);
  legacyCatalog.schemaVersion = 1;
  assert.throws(() => syncThemePacks({ ...options, manifest: legacyCatalog }), /schemaVersion 2/);
});

test("the input digest changes with catalog, CSS, font, and sync implementation bytes", () => {
  const options = fixture();
  const implementationPath = join(options.sourceRoot, "sync.mjs");
  writeFileSync(implementationPath, "version one\n");
  const digestOptions = { ...options, implementationPath };
  const initial = themePackInputDigest(digestOptions);

  writeFileSync(join(options.sourceRoot, "ink", "pack.css"), '@font-face { font-family: "Fixture"; src: url("./fonts/fixture.woff2") }\nbody {}\n');
  const cssChanged = themePackInputDigest(digestOptions);
  assert.notEqual(cssChanged, initial);

  writeFileSync(join(options.sourceRoot, "ink", "fonts", "fixture.woff2"), Buffer.from([9, 8, 7]));
  const fontChanged = themePackInputDigest(digestOptions);
  assert.notEqual(fontChanged, cssChanged);

  const catalogChanged = structuredClone(options.manifest);
  catalogChanged.packs[1].meta.version = "2.3.5";
  writeFileSync(join(options.sourceRoot, "ink", "meta.json"), JSON.stringify(catalogChanged.packs[1].meta));
  const metadataChanged = themePackInputDigest({ ...digestOptions, manifest: catalogChanged });
  assert.notEqual(metadataChanged, fontChanged);

  writeFileSync(implementationPath, "version two\n");
  assert.notEqual(themePackInputDigest({ ...digestOptions, manifest: catalogChanged }), metadataChanged);
});

test("validation rejects malformed metadata and font declarations", () => {
  const options = fixture();
  const invalid = structuredClone(options.manifest);
  invalid.packs[1].meta.preview.dark.syntax.keyword = "";
  writeFileSync(join(options.sourceRoot, "ink", "meta.json"), JSON.stringify(invalid.packs[1].meta));
  assert.throws(() => syncThemePacks({ ...options, manifest: invalid }), /invalid dark syntax preview metadata/);

  writeFileSync(join(options.sourceRoot, "ink", "meta.json"), JSON.stringify(options.manifest.packs[1].meta));
  writeFileSync(join(options.sourceRoot, "ink", "pack.css"), '@font-face { font-family: "Other"; src: url("./fonts/fixture.woff2") }\n');
  assert.throws(() => syncThemePacks(options), /fonts.loaded has no matching @font-face: Fixture/);
});
