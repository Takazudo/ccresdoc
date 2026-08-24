#!/usr/bin/env node

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  chmodSync,
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import {
  themePackInputDigest,
  validateGeneratedThemePacks,
} from "../app/scripts/sync-theme-packs.mjs";
import {
  assertAllowlistedInventory,
  assertRuntimeWorkspacePrivacy,
  copyGeneratedThemePacks,
  copyRuntimeApp,
} from "./runtime-workspace-files.mjs";
import {
  RUNTIME_WORKSPACE_DIGEST_ALGORITHM,
  refreshTokenFromWorkspaceDigest,
  runtimeWorkspaceDigest,
} from "./runtime-workspace-digest.mjs";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const appRoot = join(repoRoot, "app");
const stageRoot = join(repoRoot, "src-tauri", "runtime-workspace");
const stageApp = join(stageRoot, "app");
const packageFactsPath = join(
  repoRoot,
  "compatibility",
  "node-free-latest",
  "evidence",
  "package-facts.json",
);

const platformPackages = {
  "darwin-arm64": { package: "@takazudo/zfb-darwin-arm64", factKey: "darwin-arm64" },
  "darwin-x64": { package: "@takazudo/zfb-darwin-x64", factKey: "darwin-x64" },
  "linux-arm64": { package: "@takazudo/zfb-linux-arm64-gnu", factKey: "linux-arm64-gnu" },
  "linux-x64": { package: "@takazudo/zfb-linux-x64-gnu", factKey: "linux-x64-gnu" },
  "win32-x64": { package: "@takazudo/zfb-win32-x64-msvc", factKey: "win32-x64-msvc" },
};
const hostPlatform = platformPackages[`${process.platform}-${process.arch}`];
assert(hostPlatform, `unsupported packaging host: ${process.platform}-${process.arch}`);
const hostPackage = hostPlatform.package;
const packageFacts = JSON.parse(readFileSync(packageFactsPath, "utf8"));
const hostNativeFact = packageFacts.nativeCarriers[hostPlatform.factKey];
assert(hostNativeFact, `canonical native fact missing: ${hostPlatform.factKey}`);

const packageJson = JSON.parse(readFileSync(join(appRoot, "package.json"), "utf8"));
assert.equal(
  hostNativeFact.package,
  `${hostPackage}@${packageJson.optionalDependencies?.[hostPackage] ?? ""}`,
  `canonical native package drift: ${hostPackage}`,
);
const themeAssets = validateGeneratedThemePacks();
const packageQueue = Object.keys(packageJson.dependencies ?? {});
const packages = new Map();
const skippedZudoDocBuildOnly = new Set();

function sourcePackageDir(name) {
  const candidate = join(appRoot, "node_modules", ...name.split("/"));
  assert(existsSync(join(candidate, "package.json")), `installed package missing: ${name}`);
  return realpathSync(candidate);
}

while (packageQueue.length > 0) {
  const name = packageQueue.shift();
  if (packages.has(name)) continue;
  const source = sourcePackageDir(name);
  const manifest = JSON.parse(readFileSync(join(source, "package.json"), "utf8"));
  packages.set(name, { source, version: manifest.version });

  // zudo-doc's declared dependencies power its Node CLI/scaffolding and its
  // disabled plugin hosts. The selected configuration uses package exports,
  // CSS and Preact components only; the staged-copy runtime probe is the gate
  // that makes this edge-level pruning safe.
  const dependencies = name === "@takazudo/zudo-doc" ? {} : manifest.dependencies ?? {};
  if (name === "@takazudo/zudo-doc") {
    for (const dependency of Object.keys(manifest.dependencies ?? {})) {
      skippedZudoDocBuildOnly.add(dependency);
    }
  }
  for (const dependency of Object.keys(dependencies)) packageQueue.push(dependency);

  if (name === "@takazudo/zfb") packageQueue.push(hostPackage);
}

assert(!stageRoot.startsWith(`${appRoot}${sep}`), "stage destination must not be inside app/");
rmSync(stageRoot, { recursive: true, force: true });
mkdirSync(stageApp, { recursive: true });

const copiedApp = copyRuntimeApp(appRoot, stageApp);
const copiedThemeFiles = copyGeneratedThemePacks(
  appRoot,
  join(stageApp, "public", "theme-packs"),
);
validateGeneratedThemePacks({ outputRoot: join(stageApp, "public", "theme-packs") });
assertAllowlistedInventory(stageApp);
const privacyAudit = assertRuntimeWorkspacePrivacy(stageApp, {
  // These are build checkout paths, not user-resource paths.  Rejecting them
  // catches accidental sourcemaps or fixture copies without embedding them in
  // the staged artifact.
  forbiddenPaths: [repoRoot, appRoot],
});

const runtimePackageJson = {
  name: packageJson.name,
  version: packageJson.version,
  private: true,
  type: packageJson.type,
  engines: packageJson.engines,
  dependencies: packageJson.dependencies,
  optionalDependencies: { [hostPackage]: packageJson.optionalDependencies[hostPackage] },
};
writeFileSync(join(stageApp, "package.json"), `${JSON.stringify(runtimePackageJson, null, 2)}\n`);

for (const [name, detail] of packages) {
  const destination = join(stageApp, "node_modules", ...name.split("/"));
  mkdirSync(dirname(destination), { recursive: true });
  cpSync(detail.source, destination, { recursive: true, dereference: true });
}

const binaryName = process.platform === "win32" ? "zfb.exe" : "zfb";
const nativeBinary = join(stageApp, "node_modules", ...hostPackage.split("/"), binaryName);
assert(existsSync(nativeBinary), `host native binary missing: ${nativeBinary}`);
if (process.platform !== "win32") {
  chmodSync(nativeBinary, statSync(nativeBinary).mode | 0o755);
  assert((statSync(nativeBinary).mode & 0o111) !== 0, "native zfb binary must remain executable");
}
assert.equal(statSync(nativeBinary).size, hostNativeFact.sizeBytes, "native zfb size must match canonical facts");
assert.equal(
  createHash("sha256").update(readFileSync(nativeBinary)).digest("hex"),
  hostNativeFact.sha256,
  "native zfb digest must match canonical facts",
);

const themeAssetsDigest = themePackInputDigest();

const sourceConfig = readFileSync(join(appRoot, "zfb.config.ts"), "utf8");
// Embed the complete generated-theme input digest as a harmless config comment
// so a sync-implementation or package-source change participates even when it
// happens to produce byte-identical public assets.
const stagedConfig = `${sourceConfig.trimEnd()}\n// staged theme-assets digest: ${themeAssetsDigest}\n`;
writeFileSync(join(stageApp, "zfb.config.ts"), stagedConfig);
const workspaceDigest = runtimeWorkspaceDigest(stageApp, {
  implementationFiles: [
    {
      label: "scripts/runtime-workspace-digest.mjs",
      path: fileURLToPath(new URL("./runtime-workspace-digest.mjs", import.meta.url)),
    },
    {
      label: "scripts/stage-runtime-workspace.mjs",
      path: fileURLToPath(import.meta.url),
    },
    {
      label: "scripts/runtime-workspace-files.mjs",
      path: fileURLToPath(new URL("./runtime-workspace-files.mjs", import.meta.url)),
    },
  ],
});
const token = refreshTokenFromWorkspaceDigest(workspaceDigest);
writeFileSync(join(stageRoot, "version.txt"), `${token}\n`);

const stagedNames = [...packages.keys()].sort();
const manifest = {
  schemaVersion: 1,
  source: "src-tauri/runtime-workspace/app",
  refreshToken: token,
  host: `${process.platform}-${process.arch}`,
  hostPackage,
  platformPackages: Object.fromEntries(
    Object.entries(platformPackages).map(([platform, detail]) => [platform, detail.package]),
  ),
  nativeBinary: relative(stageRoot, nativeBinary),
  workspaceDigest: {
    algorithm: RUNTIME_WORKSPACE_DIGEST_ALGORITHM,
    value: workspaceDigest,
  },
  themeAssets: {
    digest: themeAssetsDigest,
    packs: themeAssets.packs,
    files: themeAssets.files,
    publicRoot: "app/public/theme-packs",
  },
  admittedAppFiles: {
    source: copiedApp.copiedSourceFiles,
    dist: copiedApp.copiedDistFiles,
    theme: copiedThemeFiles.map((path) => `public/theme-packs/${path}`),
  },
  privacy: {
    audit: "staged-app-surfaces",
    filesChecked: privacyAudit.filesChecked,
    sentinelsChecked: privacyAudit.sentinelsChecked,
    generatedResourceDetails: "excluded-by-file-and-route-allowlist",
    configuredRootPaths: "rejected-from-staged-text-surfaces",
  },
  packages: stagedNames.map((name) => ({ name, version: packages.get(name).version })),
  excluded: {
    appEntries: ["test", "vitest.config.ts", ".zfb", ".zfb-build", "node_modules/.bin"],
    developmentPackages: Object.keys(packageJson.devDependencies ?? {}).sort(),
    nonHostPlatformPackages: Object.values(platformPackages)
      .map(({ package: name }) => name)
      .filter((name) => name !== hostPackage),
    disabledZudoDocNodePackages: [...skippedZudoDocBuildOnly].sort(),
    forbiddenRuntimePackages: [
      "@takazudo/zfb-adapter-cloudflare",
      "@takazudo/zdtp",
      "@takazudo/zudo-doc-history-server",
      "diff",
      "esbuild",
      "smol-toml",
    ],
  },
};
writeFileSync(join(stageRoot, "runtime-manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);

console.log(`staged ${stagedNames.length} runtime packages for ${manifest.host} (${token})`);
