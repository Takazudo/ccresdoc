#!/usr/bin/env node

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  RUNTIME_APP_FILES,
  assertAllowlistedInventory,
  assertRuntimeWorkspacePrivacy,
} from "./runtime-workspace-files.mjs";
import {
  RUNTIME_WORKSPACE_DIGEST_ALGORITHM,
  refreshTokenFromWorkspaceDigest,
  runtimeWorkspaceDigest,
} from "./runtime-workspace-digest.mjs";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const stageRoot = join(repoRoot, "src-tauri", "runtime-workspace");
const stageApp = join(stageRoot, "app");
const packageFacts = JSON.parse(readFileSync(join(
  repoRoot,
  "compatibility",
  "node-free-latest",
  "evidence",
  "package-facts.json",
), "utf8"));
const manifest = JSON.parse(readFileSync(join(stageRoot, "runtime-manifest.json"), "utf8"));
const names = new Set(manifest.packages.map(({ name }) => name));
const expectedPlatformPackages = {
  "darwin-arm64": "@takazudo/zfb-darwin-arm64",
  "darwin-x64": "@takazudo/zfb-darwin-x64",
  "linux-arm64": "@takazudo/zfb-linux-arm64-gnu",
  "linux-x64": "@takazudo/zfb-linux-x64-gnu",
  "win32-x64": "@takazudo/zfb-win32-x64-msvc",
};
assert.deepEqual(manifest.platformPackages, expectedPlatformPackages, "five-platform native map drifted");
assert.deepEqual(
  manifest.admittedAppFiles.source,
  RUNTIME_APP_FILES,
  "runtime source inventory must match the checked-in allowlist",
);
assert.deepEqual(manifest.admittedAppFiles.dist, [], "runtime dist must remain omitted from the package");
assert(manifest.admittedAppFiles.theme.length > 0, "runtime theme catalog/assets must be admitted explicitly");

for (const entry of ["package.json", "pnpm-lock.yaml", "zfb.config.ts", "pages", "src", "node_modules"]) {
  assert(existsSync(join(stageApp, entry)), `runtime entry missing: ${entry}`);
}
assert(!existsSync(join(stageApp, "dist")), "runtime dist must not be copied from the build checkout");
assertAllowlistedInventory(stageApp);
const privacyAudit = assertRuntimeWorkspacePrivacy(stageApp, {
  forbiddenPaths: [repoRoot, join(repoRoot, "app")],
});
assert.equal(manifest.privacy.audit, "staged-app-surfaces");
assert.equal(manifest.privacy.filesChecked, privacyAudit.filesChecked);
assert.equal(manifest.privacy.sentinelsChecked, privacyAudit.sentinelsChecked);
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
for (const forbidden of ["esbuild", "smol-toml"]) {
  assert(
    manifest.excluded.forbiddenRuntimePackages.includes(forbidden),
    `Node-only package must be an explicit runtime exclusion: ${forbidden}`,
  );
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
const hostFact = Object.values(packageFacts.nativeCarriers).find(
  ({ package: packageId }) => packageId === `${manifest.hostPackage}@${runtimePackageJson.optionalDependencies[manifest.hostPackage]}`,
);
assert(hostFact, `canonical native fact missing for ${manifest.hostPackage}`);
assert.equal(manifest.nativeBinary, join("app", hostFact.relativePath));
assert.equal(statSync(binary).size, hostFact.sizeBytes, "native binary size differs from canonical facts");
assert.equal(
  createHash("sha256").update(readFileSync(binary)).digest("hex"),
  hostFact.sha256,
  "native binary digest differs from canonical facts",
);

const config = readFileSync(join(stageApp, "zfb.config.ts"), "utf8");
assert.match(config, /plugins:\s*\[\s*\]/, "selected runtime config must force an empty plugin list");
const workspaceDigest = runtimeWorkspaceDigest(stageApp, {
  implementationFiles: [
    {
      label: "scripts/runtime-workspace-digest.mjs",
      path: fileURLToPath(new URL("./runtime-workspace-digest.mjs", import.meta.url)),
    },
    {
      label: "scripts/stage-runtime-workspace.mjs",
      path: fileURLToPath(new URL("./stage-runtime-workspace.mjs", import.meta.url)),
    },
    {
      label: "scripts/runtime-workspace-files.mjs",
      path: fileURLToPath(new URL("./runtime-workspace-files.mjs", import.meta.url)),
    },
  ],
});
assert.equal(manifest.workspaceDigest.algorithm, RUNTIME_WORKSPACE_DIGEST_ALGORITHM);
assert.equal(manifest.workspaceDigest.value, workspaceDigest);
const expectedToken = refreshTokenFromWorkspaceDigest(workspaceDigest);
assert.equal(readFileSync(join(stageRoot, "version.txt"), "utf8").trim(), expectedToken);
assert.equal(manifest.refreshToken, expectedToken);

const darwinArm64 = packageFacts.nativeCarriers["darwin-arm64"];
assert.equal(
  darwinArm64.package,
  `@takazudo/zfb-darwin-arm64@${packageFacts.packages["@takazudo/zfb"].version}`,
);
assert.equal(darwinArm64.relativePath, "node_modules/@takazudo/zfb-darwin-arm64/zfb");
assert.match(darwinArm64.sha256, /^[a-f0-9]{64}$/);
assert(Number.isSafeInteger(darwinArm64.sizeBytes) && darwinArm64.sizeBytes > 0);
const appLockfile = readFileSync(join(repoRoot, "app", "pnpm-lock.yaml"), "utf8");
assert(
  appLockfile.includes(
    `  '${darwinArm64.package}':\n    resolution: {integrity: ${darwinArm64.integrity}}`,
  ),
  "Darwin-arm64 published carrier integrity must match the canonical facts",
);

console.log(JSON.stringify({
  status: "passed",
  ...manifest,
  darwinArm64PublishedCarrier: {
    verification: "canonical-facts-and-lockfile-static-check",
    package: darwinArm64.package,
    integrity: darwinArm64.integrity,
    sizeBytes: darwinArm64.sizeBytes,
    sha256: darwinArm64.sha256,
    macosAppWebViewHostGate: "not-run-by-this-static-check",
  },
}, null, 2));
