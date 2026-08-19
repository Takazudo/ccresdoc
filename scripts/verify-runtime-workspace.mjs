#!/usr/bin/env node

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const stageRoot = join(repoRoot, "src-tauri", "runtime-workspace");
const stageApp = join(stageRoot, "app");
const manifest = JSON.parse(readFileSync(join(stageRoot, "runtime-manifest.json"), "utf8"));
const names = new Set(manifest.packages.map(({ name }) => name));

for (const entry of ["package.json", "pnpm-lock.yaml", "zfb.config.ts", "pages", "src", "node_modules"]) {
  assert(existsSync(join(stageApp, entry)), `runtime entry missing: ${entry}`);
}
for (const entry of manifest.excluded.appEntries) {
  assert(!existsSync(join(stageApp, entry)), `excluded app entry was staged: ${entry}`);
}
for (const name of [
  ...manifest.excluded.developmentPackages,
  ...manifest.excluded.nonHostPlatformPackages,
  ...manifest.excluded.disabledZudoDocNodePackages,
  ...manifest.excluded.forbiddenRuntimePackages,
]) {
  assert(!names.has(name), `excluded package was staged: ${name}`);
  assert(!existsSync(join(stageApp, "node_modules", ...name.split("/"))), `excluded package exists: ${name}`);
}

const runtimePackageJson = JSON.parse(readFileSync(join(stageApp, "package.json"), "utf8"));
assert.equal(runtimePackageJson.devDependencies, undefined, "staged package.json must omit devDependencies");
assert.deepEqual(Object.keys(runtimePackageJson.optionalDependencies), [manifest.hostPackage]);

for (const required of [
  "@takazudo/zfb",
  "@takazudo/zfb-md-wasm",
  "@takazudo/zfb-runtime",
  "@takazudo/zudo-doc",
  "preact",
  "preact-render-to-string",
  "zod",
  "katex",
  manifest.hostPackage,
]) {
  assert(names.has(required), `required runtime package was not staged: ${required}`);
}

const binary = join(stageRoot, manifest.nativeBinary);
assert(existsSync(binary), `native binary missing: ${binary}`);
if (process.platform !== "win32") assert((statSync(binary).mode & 0o111) !== 0, "native binary is not executable");
assert(!manifest.nativeBinary.includes("node_modules/.bin"), "must resolve the platform binary directly");

const config = readFileSync(join(stageApp, "zfb.config.ts"), "utf8");
assert.match(config, /plugins:\s*\[\s*\]/, "selected runtime config must force an empty plugin list");
const lockfile = readFileSync(join(stageApp, "pnpm-lock.yaml"));
const expectedToken = createHash("sha256").update(lockfile).update(config).digest("hex").slice(0, 32);
assert.equal(readFileSync(join(stageRoot, "version.txt"), "utf8").trim(), expectedToken);
assert.equal(manifest.refreshToken, expectedToken);

if (process.platform === "darwin" && process.arch === "arm64") {
  assert.equal(manifest.hostPackage, "@takazudo/zfb-darwin-arm64");
  assert.equal(statSync(binary).size, 173246016);
  const digest = createHash("sha256").update(readFileSync(binary)).digest("hex");
  assert.equal(digest, "35bfa2b2cf8ffc6b5ddefdf712155e02ad6aa5e947ffcf41ee57f8e48ff2d7a0");
}

console.log(JSON.stringify({ status: "passed", ...manifest }, null, 2));
