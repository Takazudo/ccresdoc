import assert from "node:assert/strict";
import {
  cpSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  statSync,
} from "node:fs";
import { dirname, join, relative, sep } from "node:path";

// This is intentionally a file-level allowlist.  The app source tree also
// contains tests and, after a launch, generated resource details.  A recursive
// copy of `src/` or `dist/` would make those local bytes part of a release.
export const RUNTIME_APP_FILES = Object.freeze([
  "package.json",
  "pnpm-lock.yaml",
  "pnpm-workspace.yaml",
  "patches/@takazudo__zudo-doc@5.12.1.patch",
  "tsconfig.json",
  "zfb.config.ts",
  "pages/404.tsx",
  "pages/index.tsx",
  "pages/docs/[[...slug]].tsx",
  "pages/lib/_chrome.ts",
  "pages/lib/_route-context.ts",
  "pages/lib/_settings-button.ts",
  "src/appearance/bridge.tsx",
  "src/browser-chrome/command-catalog.json",
  "src/browser-chrome/adapter.ts",
  "src/browser-chrome/history.ts",
  "src/browser-chrome/toolbar.tsx",
  "src/browser-chrome/types.ts",
  "src/config/settings.ts",
  "src/config/theme-pack-slugs.json",
  "src/content/docs/index.mdx",
  "src/content/docs/welcome.mdx",
  "src/content/docs/claude/index.mdx",
  "src/content/docs/codex/index.mdx",
  "src/styles/global.css",
]);

// zfb's built output has content-addressed assets, so the allowlist for `dist`
// is structural: static assets and theme files are allowed, while only the
// coordinator-owned landing/error routes may be copied. Every generated
// detail/status route is rejected before it can reach the staged tree.
export const RUNTIME_DIST_ROUTE_FILES = Object.freeze([
  "404.html",
  "index.html",
  "docs/index.html",
  "docs/claude/index.html",
  "docs/codex/index.html",
]);

// `claude/` and `codex/` are the coordinator-owned generic landing inputs.
// Only their hyphenated detail/status namespaces are generator-owned.
export const GENERATED_RESOURCE_NAMESPACE_PATTERN = /^(?:claude|codex)-/;
export const GENERATED_RUNTIME_NAMESPACE_PATTERN = /^\.ccresdoc(?:-|$)/;

// These strings are deliberately synthetic and public-repository-specific.
// They let staging and bundle probes catch accidental reuse of the settings
// and packaged-app fixtures without naming or copying any user's resources.
export const PRIVACY_SENTINELS = Object.freeze([
  "ccresdoc-private-source",
  "ccresdoc-private-resource",
  "fixture-claude",
  "fixture-codex",
  "Package Readiness Probe",
  "Authored source fixture marker",
  "Generated package route",
]);

function portable(path) {
  return path.split(sep).join("/");
}

function copyFile(sourceRoot, destinationRoot, relativePath) {
  const source = join(sourceRoot, relativePath);
  assert(existsSync(source), `runtime allowlist source is missing: ${source}`);
  assert(lstatSync(source).isFile(), `runtime allowlist source is not a file: ${source}`);
  const destination = join(destinationRoot, relativePath);
  mkdirSync(dirname(destination), { recursive: true });
  cpSync(source, destination, { recursive: false, dereference: true });
}

function walkFiles(root, current = root) {
  if (!existsSync(root)) return [];
  const files = [];
  for (const entry of readdirSync(current, { withFileTypes: true })) {
    const path = join(current, entry.name);
    if (entry.isDirectory()) files.push(...walkFiles(root, path));
    else if (entry.isFile()) files.push(portable(relative(root, path)));
    else assert.fail(`runtime workspace contains unsupported entry: ${path}`);
  }
  return files.sort();
}

function allowDistFile(relativePath) {
  if (RUNTIME_DIST_ROUTE_FILES.includes(relativePath)) return true;
  return relativePath.startsWith("assets/")
    || relativePath.startsWith("theme-packs/")
    || relativePath === "__zfb/routes.json";
}

function copyDist(sourceRoot, destinationRoot) {
  const sourceDist = join(sourceRoot, "dist");
  if (!existsSync(sourceDist)) return [];
  assert(statSync(sourceDist).isDirectory(), `runtime dist source is not a directory: ${sourceDist}`);
  const copied = [];
  for (const relativePath of walkFiles(sourceDist)) {
    assert(allowDistFile(relativePath), `runtime dist path is not allowlisted: ${relativePath}`);
    copyFile(join(sourceRoot, "dist"), join(destinationRoot, "dist"), relativePath);
    copied.push(relativePath);
  }
  return copied;
}

/**
 * Copy only release-owned app inputs into a runtime workspace.
 *
 * The returned inventory is persisted in the runtime manifest so reviewers
 * can see exactly which source and built paths were admitted.
 */
export function copyRuntimeApp(sourceRoot, destinationRoot, { includeDist = false } = {}) {
  const copiedSourceFiles = [];
  for (const relativePath of RUNTIME_APP_FILES) {
    copyFile(sourceRoot, destinationRoot, relativePath);
    copiedSourceFiles.push(relativePath);
  }
  // A built dist bundle embeds absolute module paths in its island registry on
  // some hosts. The native sidecar rebuilds from the allowlisted source tree,
  // so dist is omitted from release staging by default. Tests may opt into the
  // structural route allowlist to prove generated pages cannot be admitted.
  const copiedDistFiles = includeDist ? copyDist(sourceRoot, destinationRoot) : [];
  return { copiedSourceFiles, copiedDistFiles };
}

/** Copy the generated public theme catalog/assets, and no other public files. */
export function copyGeneratedThemePacks(sourceRoot, destinationRoot) {
  const source = join(sourceRoot, "public", "theme-packs");
  assert(existsSync(source), `generated theme-pack directory is missing: ${source}`);
  assert(statSync(source).isDirectory(), `generated theme-pack path is not a directory: ${source}`);
  const files = walkFiles(source);
  assert(files.includes("index.json"), "generated theme-pack catalog is missing");
  for (const relativePath of files) {
    copyFile(source, destinationRoot, relativePath);
  }
  return files;
}

function pathHasGeneratedNamespace(relativePath) {
  const parts = portable(relativePath).split("/");
  return parts.some((part) => GENERATED_RUNTIME_NAMESPACE_PATTERN.test(part))
    || parts.some((part) => GENERATED_RESOURCE_NAMESPACE_PATTERN.test(part));
}

function textFile(path) {
  const bytes = readFileSync(path);
  // Binary assets are covered by the path/allowlist checks.  Restrict content
  // scans to valid UTF-8 to avoid treating arbitrary font/wasm bytes as prose.
  const text = bytes.toString("utf8");
  return Buffer.from(text, "utf8").equals(bytes) ? text : null;
}

function privacySurfaceFiles(root) {
  const candidates = [
    "package.json",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
    "tsconfig.json",
    "zfb.config.ts",
    "pages",
    "patches",
    "public",
    "src",
    "dist",
  ];
  const files = [];
  for (const entry of candidates) {
    const path = join(root, entry);
    if (!existsSync(path)) continue;
    if (statSync(path).isFile()) files.push(entry);
    else files.push(...walkFiles(path).map((child) => join(entry, child)));
  }
  return files;
}

/**
 * Assert that staged app inputs contain no generated resource namespace or
 * known local-fixture content. `forbiddenPaths` is supplied by the caller for
 * the current checkout and is never written into the stage.
 */
export function assertRuntimeWorkspacePrivacy(root, { forbiddenPaths = [] } = {}) {
  const files = privacySurfaceFiles(root);
  for (const relativePath of files) {
    assert(!pathHasGeneratedNamespace(relativePath), `generated resource path was staged: ${relativePath}`);
    const content = textFile(join(root, relativePath));
    if (content === null) continue;
    for (const sentinel of PRIVACY_SENTINELS) {
      assert(!content.includes(sentinel), `private fixture sentinel was staged: ${sentinel}`);
    }
    for (const forbiddenPath of forbiddenPaths) {
      if (!forbiddenPath) continue;
      assert(!content.includes(forbiddenPath), `configured checkout path was staged: ${forbiddenPath}`);
    }
  }

  return { filesChecked: files.length, sentinelsChecked: PRIVACY_SENTINELS.length };
}

export function assertRuntimeRenderedPrivacy(label, content, { forbiddenPaths = [] } = {}) {
  assert.equal(typeof content, "string", `${label} rendered response must be text`);
  for (const sentinel of PRIVACY_SENTINELS) {
    assert(!content.includes(sentinel), `${label} rendered response leaked fixture sentinel: ${sentinel}`);
  }
  for (const forbiddenPath of forbiddenPaths) {
    if (!forbiddenPath) continue;
    assert(!content.includes(forbiddenPath), `${label} rendered response leaked configured path: ${forbiddenPath}`);
  }
  return { sentinelsChecked: PRIVACY_SENTINELS.length };
}

export function assertAllowlistedInventory(root) {
  for (const relativePath of RUNTIME_APP_FILES) {
    assert(existsSync(join(root, relativePath)), `allowlisted runtime file missing: ${relativePath}`);
  }
  const sourceSurfaceFiles = [
    ...["package.json", "pnpm-lock.yaml", "pnpm-workspace.yaml", "tsconfig.json", "zfb.config.ts"],
    ...walkFiles(join(root, "patches")).map((path) => `patches/${path}`),
    ...walkFiles(join(root, "pages")).map((path) => `pages/${path}`),
    ...walkFiles(join(root, "src")).map((path) => `src/${path}`),
  ].sort();
  assert.deepEqual(
    sourceSurfaceFiles,
    [...RUNTIME_APP_FILES].sort(),
    "runtime source surfaces must match the explicit file allowlist",
  );
  for (const relativePath of walkFiles(join(root, "src", "content", "docs"))) {
    assert(
      relativePath === "index.mdx"
        || relativePath === "welcome.mdx"
        || relativePath === "claude/index.mdx"
        || relativePath === "codex/index.mdx",
      `unexpected content input in runtime workspace: ${relativePath}`,
    );
  }
  const publicFiles = walkFiles(join(root, "public"));
  for (const relativePath of publicFiles) {
    assert(
      relativePath === "theme-packs/index.json" || relativePath.startsWith("theme-packs/"),
      `unexpected public input in runtime workspace: ${relativePath}`,
    );
  }
}
