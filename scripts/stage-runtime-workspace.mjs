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

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const appRoot = join(repoRoot, "app");
const stageRoot = join(repoRoot, "src-tauri", "runtime-workspace");
const stageApp = join(stageRoot, "app");

const platformPackages = {
  "darwin-arm64": "@takazudo/zfb-darwin-arm64",
  "darwin-x64": "@takazudo/zfb-darwin-x64",
  "linux-arm64": "@takazudo/zfb-linux-arm64-gnu",
  "linux-x64": "@takazudo/zfb-linux-x64-gnu",
  "win32-x64": "@takazudo/zfb-win32-x64-msvc",
};
const hostPackage = platformPackages[`${process.platform}-${process.arch}`];
assert(hostPackage, `unsupported packaging host: ${process.platform}-${process.arch}`);

const appFiles = [
  "package.json",
  "pnpm-lock.yaml",
  "pnpm-workspace.yaml",
  "tsconfig.json",
  "zfb.config.ts",
  "pages",
  "public",
  "src",
  "dist",
];

const packageJson = JSON.parse(readFileSync(join(appRoot, "package.json"), "utf8"));
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

for (const entry of appFiles) {
  const source = join(appRoot, entry);
  if (!existsSync(source)) continue;
  cpSync(source, join(stageApp, entry), { recursive: true, dereference: true });
}
validateGeneratedThemePacks({ outputRoot: join(stageApp, "public", "theme-packs") });

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

const themeAssetsDigest = themePackInputDigest();

const lockfile = readFileSync(join(appRoot, "pnpm-lock.yaml"));
const sourceConfig = readFileSync(join(appRoot, "zfb.config.ts"), "utf8");
// The verifier intentionally derives the refresh token only from staged lock +
// config bytes. Embed the complete generated-theme input digest as a harmless
// config comment so catalog, CSS, font, or sync changes participate too.
const stagedConfig = `${sourceConfig.trimEnd()}\n// staged theme-assets digest: ${themeAssetsDigest}\n`;
writeFileSync(join(stageApp, "zfb.config.ts"), stagedConfig);
const token = createHash("sha256").update(lockfile).update(stagedConfig).digest("hex").slice(0, 32);
writeFileSync(join(stageRoot, "version.txt"), `${token}\n`);

const stagedNames = [...packages.keys()].sort();
const manifest = {
  schemaVersion: 1,
  source: "app/pnpm-lock.yaml",
  refreshToken: token,
  host: `${process.platform}-${process.arch}`,
  hostPackage,
  nativeBinary: relative(stageRoot, nativeBinary),
  themeAssets: {
    digest: themeAssetsDigest,
    packs: themeAssets.packs,
    files: themeAssets.files,
    publicRoot: "app/public/theme-packs",
  },
  packages: stagedNames.map((name) => ({ name, version: packages.get(name).version })),
  excluded: {
    appEntries: ["test", "vitest.config.ts", ".zfb", ".zfb-build", "node_modules/.bin"],
    developmentPackages: Object.keys(packageJson.devDependencies ?? {}).sort(),
    nonHostPlatformPackages: Object.values(platformPackages).filter((name) => name !== hostPackage),
    disabledZudoDocNodePackages: [...skippedZudoDocBuildOnly].sort(),
    forbiddenRuntimePackages: [
      "@takazudo/zfb-adapter-cloudflare",
      "@takazudo/zdtp",
      "@takazudo/zudo-doc-history-server",
      "diff",
    ],
  },
};
writeFileSync(join(stageRoot, "runtime-manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);

console.log(`staged ${stagedNames.length} runtime packages for ${manifest.host} (${token})`);
