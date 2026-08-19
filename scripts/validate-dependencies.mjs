import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = dirname(scriptDir);
const appRoot = join(repoRoot, "app");
const packageJson = JSON.parse(readFileSync(join(appRoot, "package.json"), "utf8"));
const lockfile = readFileSync(join(appRoot, "pnpm-lock.yaml"), "utf8");
const workspace = readFileSync(join(appRoot, "pnpm-workspace.yaml"), "utf8");
const checkInstalled = process.argv.includes("--installed");

const required = {
  dependencies: {
    "@takazudo/zfb": "2.7.1",
    "@takazudo/zfb-md-wasm": "2.7.1",
    "@takazudo/zfb-runtime": "2.7.1",
    "@takazudo/zudo-doc": "5.6.0",
    katex: "0.16.22",
    preact: "10.29.1",
    "preact-render-to-string": "6.6.7",
    zod: "4.3.6",
  },
  optionalDependencies: {
    "@takazudo/zfb-darwin-arm64": "2.7.1",
    "@takazudo/zfb-darwin-x64": "2.7.1",
    "@takazudo/zfb-linux-arm64-gnu": "2.7.1",
    "@takazudo/zfb-linux-x64-gnu": "2.7.1",
    "@takazudo/zfb-win32-x64-msvc": "2.7.1",
  },
  devDependencies: {
    "@tailwindcss/vite": "4.2.0",
    "happy-dom": "20.7.0",
    tailwindcss: "4.2.0",
    typescript: "5.9.2",
    vitest: "4.0.17",
  },
};

const errors = [];
const fail = (message) => errors.push(message);
const equal = (actual, expected, label) => {
  if (actual !== expected) fail(`${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
};
const escapeRegex = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

equal(packageJson.name, "@ccresdoc/app", "app package name");
equal(packageJson.packageManager, "pnpm@10.30.3", "app packageManager");
equal(packageJson.engines?.node, ">=22.0.0", "app engines.node");
equal(packageJson.engines?.pnpm, ">=10.0.0", "app engines.pnpm");

for (const [section, expectedEntries] of Object.entries(required)) {
  const actual = packageJson[section] ?? {};
  for (const [name, version] of Object.entries(expectedEntries)) equal(actual[name], version, `app ${section}.${name}`);
  for (const name of Object.keys(actual)) {
    if (!Object.hasOwn(expectedEntries, name)) fail(`unexpected app ${section} entry: ${name}`);
  }
}

for (const name of [
  "@takazudo/zfb-adapter-cloudflare",
  "@types/react",
  "clsx",
  "gray-matter",
  "mermaid",
  "remark-cjk-friendly",
  "remark-directive",
]) {
  for (const section of ["dependencies", "optionalDependencies", "devDependencies"]) {
    if (Object.hasOwn(packageJson[section] ?? {}, name)) fail(`forbidden direct dependency: ${name}`);
  }
}

if (!/^lockfileVersion: ['"]?9(?:\.0)?['"]?$/m.test(lockfile)) fail("lockfile must use pnpm lockfile v9");
if (!/^nodeLinker:\s+hoisted$/m.test(workspace)) fail("pnpm-workspace.yaml must set nodeLinker: hoisted");
if (!/^minimumReleaseAge:\s+1440$/m.test(workspace)) fail("pnpm-workspace.yaml must set minimumReleaseAge: 1440");
if (workspace.includes("zfb-adapter-cloudflare")) fail("workspace must not mention the removed Cloudflare adapter");

const importerSection = (section) => {
  const marker = `    ${section}:\n`;
  const start = lockfile.indexOf(marker);
  if (start < 0) return null;
  const bodyStart = start + marker.length;
  const next = lockfile.slice(bodyStart).search(/\n    [A-Za-z][^\n]*:\n/);
  return lockfile.slice(bodyStart, next < 0 ? lockfile.length : bodyStart + next);
};
const importerEntry = (sectionText, name) => {
  const lines = sectionText.split("\n");
  const key = `      ${name.startsWith("@") ? `'${name}'` : name}:`;
  const start = lines.indexOf(key);
  if (start < 0) return null;
  let end = start + 1;
  while (end < lines.length && !/^      [^ ]/.test(lines[end])) end += 1;
  return lines.slice(start + 1, end).join("\n");
};
for (const [section, entries] of Object.entries(required)) {
  const sectionText = importerSection(section);
  if (!sectionText) {
    fail(`lockfile importer is missing ${section}`);
    continue;
  }
  for (const [name, version] of Object.entries(entries)) {
    const entry = importerEntry(sectionText, name);
    if (!entry) {
      fail(`lockfile importer is missing ${section}.${name}`);
      continue;
    }
    const specifier = new RegExp("^        specifier: (.+)$", "m").exec(entry)?.[1];
    const resolved = new RegExp("^        version: (.+)$", "m").exec(entry)?.[1];
    equal(specifier, version, `lockfile ${section}.${name} specifier`);
    if (!resolved?.startsWith(version)) fail(`lockfile ${section}.${name} version does not start with ${version}: ${resolved}`);
  }
}

const lockPackageVersion = (name) => {
  const escaped = escapeRegex(name);
  const match = new RegExp(`^  (?:'${escaped}|${escaped})@([^':(]+)(?:'|\\(|:)`, "m").exec(lockfile);
  return match?.[1];
};
for (const entries of Object.values(required)) {
  for (const [name, version] of Object.entries(entries)) equal(lockPackageVersion(name), version, `lockfile package ${name}`);
}

const excluded = new Set([...workspace.matchAll(/^  - ['"]?([^'"\n]+)['"]?$/gm)].map((match) => match[1]));
for (const entries of [required.dependencies, required.optionalDependencies]) {
  for (const [name, version] of Object.entries(entries)) {
    if (name.startsWith("@takazudo/") && !excluded.has(`${name}@${version}`)) {
      fail(`workspace minimumReleaseAgeExclude is missing ${name}@${version}`);
    }
  }
}

const dependencyImporter = importerSection("dependencies") ?? "";
const importerVersion = (name) => {
  return importerEntry(dependencyImporter, name)
    ?.match(/^        version: (.+)$/m)?.[1];
};
const runtimeImporter = importerVersion("@takazudo/zfb-runtime");
if (!runtimeImporter?.includes("@takazudo/zfb@2.7.1")) fail(`zfb-runtime peer must resolve zfb@2.7.1: ${runtimeImporter ?? "missing"}`);
const zudoImporter = importerVersion("@takazudo/zudo-doc");
for (const peer of ["@takazudo/zfb-md-wasm@2.7.1", "@takazudo/zfb-runtime@2.7.1", "@takazudo/zfb@2.7.1", "katex@0.16.22", "preact@10.29.1", "zod@4.3.6"]) {
  if (!zudoImporter?.includes(peer)) fail(`zudo-doc peer must resolve ${peer}: ${zudoImporter ?? "missing"}`);
}

if (checkInstalled) {
  const packageJsonPath = (name) => join(appRoot, "node_modules", ...name.split("/"), "package.json");
  for (const [section, entries] of Object.entries(required)) {
    for (const [name, version] of Object.entries(entries)) {
      const path = packageJsonPath(name);
      if (!existsSync(path)) {
        if (section === "optionalDependencies") continue;
        fail(`installed package is missing: ${name}`);
        continue;
      }
      equal(JSON.parse(readFileSync(path, "utf8")).version, version, `installed ${name}`);
    }
  }
  const host = {
    "linux-x64": "@takazudo/zfb-linux-x64-gnu",
    "linux-arm64": "@takazudo/zfb-linux-arm64-gnu",
    "darwin-arm64": "@takazudo/zfb-darwin-arm64",
    "darwin-x64": "@takazudo/zfb-darwin-x64",
    "win32-x64": "@takazudo/zfb-win32-x64-msvc",
  }[`${process.platform}-${process.arch}`];
  if (host) {
    const binaryPath = join(appRoot, "node_modules", ...host.split("/"), process.platform === "win32" ? "zfb.exe" : "zfb");
    if (!existsSync(binaryPath)) fail(`installed native zfb binary is missing: ${binaryPath}`);
    else if (process.platform !== "win32" && (statSync(binaryPath).mode & 0o111) === 0) fail(`installed native zfb binary is not executable: ${binaryPath}`);
  }
}

if (errors.length > 0) {
  console.error("dependency validation failed:");
  for (const message of errors) console.error(`- ${message}`);
  process.exit(1);
}
console.log(`dependency validation passed (${checkInstalled ? "manifest, lockfile, peers, and installed tree" : "manifest, lockfile, peers"})`);
