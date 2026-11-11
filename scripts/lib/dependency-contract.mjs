import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import {
  FORBIDDEN_DIRECT_DEPENDENCIES,
  MINIMUM_RELEASE_AGE,
  NATIVE_CARRIERS,
  NODE_ENGINE,
  PACKAGE_MANAGER,
  PNPM_ENGINE,
  RELEASE_AGE_EXCLUDES,
  RUNTIME_DEPENDENCIES,
  SCOPES,
  ZFB_VERSION,
} from "./toolchain-contract.mjs";

const escapeRegex = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
const unquote = (value) => value.trim().replace(/^(['"])(.*)\1$/, "$2");

function topLevelBlock(text, key) {
  const lines = text.split("\n");
  const start = lines.findIndex((line) => line === `${key}:`);
  if (start < 0) return null;
  const body = [];
  for (const line of lines.slice(start + 1)) {
    if (line !== "" && !line.startsWith(" ")) break;
    body.push(line);
  }
  return body;
}

export function parseWorkspace(text) {
  const exclude = (topLevelBlock(text, "minimumReleaseAgeExclude") ?? [])
    .map((line) => /^\s+-\s+(.+)$/.exec(line)?.[1])
    .filter(Boolean)
    .map(unquote);
  const patches = Object.fromEntries(
    (topLevelBlock(text, "patchedDependencies") ?? [])
      .map((line) => /^\s+(.+?):\s+(.+)$/.exec(line))
      .filter(Boolean)
      .map((match) => [unquote(match[1]), unquote(match[2])]),
  );
  return {
    nodeLinker: /^nodeLinker:\s*(\S+)\s*$/m.exec(text)?.[1] ?? null,
    minimumReleaseAge: Number(/^minimumReleaseAge:\s*(\d+)\s*$/m.exec(text)?.[1] ?? Number.NaN),
    exclude,
    patches,
  };
}

function importerSection(lockfile, section) {
  const importers = lockfile.indexOf("\nimporters:\n");
  if (importers < 0) return null;
  const marker = `\n    ${section}:\n`;
  const start = lockfile.indexOf(marker, importers);
  const packages = lockfile.indexOf("\npackages:\n", importers);
  if (start < 0 || (packages >= 0 && start > packages)) return null;
  const bodyStart = start + marker.length;
  const next = lockfile.slice(bodyStart).search(/\n(?:    [A-Za-z][^\n]*:|[A-Za-z][^\n]*:)\n/);
  return lockfile.slice(bodyStart, next < 0 ? lockfile.length : bodyStart + next);
}

function importerEntry(sectionText, name) {
  const lines = sectionText.split("\n");
  const start = lines.indexOf(`      ${name.startsWith("@") ? `'${name}'` : name}:`);
  if (start < 0) return null;
  let end = start + 1;
  while (end < lines.length && /^ {8}/.test(lines[end])) end += 1;
  const body = lines.slice(start + 1, end).join("\n");
  return {
    specifier: /^ {8}specifier: (.+)$/m.exec(body)?.[1] ?? null,
    version: /^ {8}version: (.+)$/m.exec(body)?.[1] ?? null,
  };
}

function lockedPackageVersions(lockfile, name) {
  const escaped = escapeRegex(name);
  return [...lockfile.matchAll(new RegExp(`^  '?${escaped}@([^'():]+)`, "gm"))].map((match) => match[1]);
}

/**
 * Check one install scope's manifest, workspace policy and lockfile against
 * the audited contract. Pure: every input is passed in, so negative cases can
 * be tested without touching the repository.
 */
export function checkScope(scopeKey, { packageJson, lockfile, workspace, readPatch }) {
  const scope = SCOPES[scopeKey];
  if (!scope) throw new Error(`unknown dependency scope: ${scopeKey}`);
  const errors = [];
  const label = (text) => `${scopeKey} ${text}`;
  const equal = (actual, expected, text) => {
    if (actual !== expected) errors.push(`${label(text)}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  };

  equal(packageJson.name, scope.name, "package name");
  equal(packageJson.packageManager, PACKAGE_MANAGER, "packageManager");
  equal(packageJson.engines?.node, NODE_ENGINE, "engines.node");
  equal(packageJson.engines?.pnpm, PNPM_ENGINE, "engines.pnpm");

  for (const [section, expected] of Object.entries(scope.sections)) {
    const actual = packageJson[section] ?? {};
    for (const [name, version] of Object.entries(expected)) equal(actual[name], version, `${section}.${name}`);
    for (const name of Object.keys(actual)) {
      if (!Object.hasOwn(expected, name)) errors.push(label(`has unexpected ${section} entry: ${name}`));
    }
  }
  for (const name of FORBIDDEN_DIRECT_DEPENDENCIES) {
    for (const section of ["dependencies", "optionalDependencies", "devDependencies", "peerDependencies"]) {
      if (Object.hasOwn(packageJson[section] ?? {}, name)) errors.push(label(`declares forbidden ${section} entry: ${name}`));
    }
  }

  const policy = parseWorkspace(workspace);
  equal(policy.nodeLinker, "hoisted", "workspace nodeLinker");
  equal(policy.minimumReleaseAge, MINIMUM_RELEASE_AGE, "workspace minimumReleaseAge");
  equal(JSON.stringify([...policy.exclude].sort()), JSON.stringify([...RELEASE_AGE_EXCLUDES].sort()), "workspace minimumReleaseAgeExclude");

  const expectedPatches = Object.fromEntries(scope.patches.map((patch) => [`${patch.name}@${patch.version}`, patch.path]));
  equal(JSON.stringify(policy.patches), JSON.stringify(expectedPatches), "workspace patchedDependencies");
  for (const patch of scope.patches) {
    const bytes = readPatch(patch.path);
    if (!bytes) errors.push(label(`patch is missing: ${patch.path}`));
    else equal(createHash("sha256").update(bytes).digest("hex"), patch.hash, `${patch.path} sha256`);
    if (!lockfile.includes(`hash: ${patch.hash}`) || !lockfile.includes(`path: ${patch.path}`)) {
      errors.push(label(`lockfile patchedDependencies entry drifted for ${patch.name}@${patch.version}`));
    }
  }
  if (scope.patches.length === 0 && /^patchedDependencies:/m.test(lockfile)) {
    errors.push(label("lockfile must not record patchedDependencies"));
  }

  if (!/^lockfileVersion: ['"]?9(?:\.0)?['"]?$/m.test(lockfile)) errors.push(label("lockfile must use pnpm lockfile v9"));
  for (const [section, expected] of Object.entries(scope.sections)) {
    const sectionText = importerSection(lockfile, section);
    if (!sectionText) {
      errors.push(label(`lockfile importer is missing ${section}`));
      continue;
    }
    for (const [name, version] of Object.entries(expected)) {
      const entry = importerEntry(sectionText, name);
      if (!entry) {
        errors.push(label(`lockfile importer is missing ${section}.${name}`));
        continue;
      }
      equal(entry.specifier, version, `lockfile ${section}.${name} specifier`);
      if (!entry.version?.startsWith(version)) errors.push(label(`lockfile ${section}.${name} resolves ${entry.version}, not ${version}`));
    }
  }
  for (const expected of Object.values(scope.sections)) {
    for (const [name, version] of Object.entries(expected)) {
      const locked = [...new Set(lockedPackageVersions(lockfile, name))];
      equal(JSON.stringify(locked), JSON.stringify([version]), `lockfile package versions for ${name}`);
    }
  }

  const zudoImporter = importerEntry(importerSection(lockfile, "dependencies") ?? "", "@takazudo/zudo-doc")?.version ?? "";
  const zudoPatch = scope.patches.find((patch) => patch.name === "@takazudo/zudo-doc");
  if (zudoPatch && !zudoImporter.includes(`patch_hash=${zudoPatch.hash}`)) {
    errors.push(label(`zudo-doc importer must resolve the committed patch hash: ${zudoImporter || "missing"}`));
  }
  if (!zudoPatch && zudoImporter.includes("patch_hash=")) errors.push(label("zudo-doc importer must stay unpatched"));
  for (const [name, version] of Object.entries(RUNTIME_DEPENDENCIES)) {
    if (name === "@takazudo/zudo-doc") continue;
    if (!zudoImporter.includes(`${name}@${version}`)) errors.push(label(`zudo-doc peer must resolve ${name}@${version}`));
  }
  return errors;
}

/** Both scopes must declare the same runtime tuple and carriers. */
export function checkScopesAgree(appPackageJson, probePackageJson) {
  const errors = [];
  for (const section of ["dependencies", "optionalDependencies"]) {
    const app = JSON.stringify(appPackageJson[section] ?? {}, Object.keys(appPackageJson[section] ?? {}).sort());
    const probe = JSON.stringify(probePackageJson[section] ?? {}, Object.keys(probePackageJson[section] ?? {}).sort());
    if (app !== probe) errors.push(`app and probe ${section} disagree: ${app} vs ${probe}`);
  }
  for (const field of ["packageManager"]) {
    if (appPackageJson[field] !== probePackageJson[field]) errors.push(`app and probe ${field} disagree`);
  }
  if (appPackageJson.engines?.node !== probePackageJson.engines?.node) errors.push("app and probe engines.node disagree");
  return errors;
}

/** Installed-tree check: versions, the host's native carrier, and patch application. */
export function checkInstalled(scopeKey, root, { platform = process.platform, arch = process.arch } = {}) {
  const scope = SCOPES[scopeKey];
  const errors = [];
  for (const [section, expected] of Object.entries(scope.sections)) {
    for (const [name, version] of Object.entries(expected)) {
      const manifest = join(root, "node_modules", ...name.split("/"), "package.json");
      if (!existsSync(manifest)) {
        if (section !== "optionalDependencies") errors.push(`${scopeKey} installed package is missing: ${name}`);
        continue;
      }
      const installed = JSON.parse(readFileSync(manifest, "utf8")).version;
      if (installed !== version) errors.push(`${scopeKey} installed ${name}: expected ${version}, got ${installed}`);
    }
  }
  const carrier = NATIVE_CARRIERS[`${platform}-${arch}`];
  if (!carrier) errors.push(`${scopeKey} has no native carrier for ${platform}-${arch}`);
  else {
    const binary = join(root, "node_modules", ...carrier.split("/"), platform === "win32" ? "zfb.exe" : "zfb");
    if (!existsSync(binary)) errors.push(`${scopeKey} installed native zfb binary is missing: ${binary}`);
    else if (platform !== "win32" && (statSync(binary).mode & 0o111) === 0) errors.push(`${scopeKey} native zfb binary is not executable: ${binary}`);
    else {
      const carrierVersion = JSON.parse(readFileSync(join(root, "node_modules", ...carrier.split("/"), "package.json"), "utf8")).version;
      if (carrierVersion !== ZFB_VERSION) errors.push(`${scopeKey} host carrier ${carrier} is ${carrierVersion}, not ${ZFB_VERSION}`);
    }
  }
  const zudoDist = join(root, "node_modules", "@takazudo", "zudo-doc", "dist");
  const read = (...parts) => {
    const path = join(zudoDist, ...parts);
    return existsSync(path) ? readFileSync(path, "utf8") : "";
  };
  const find = read("find-in-page", "index.js");
  const search = read("search-widget-script", "index.js");
  const zdtp = read("zdtp-loader.js");
  const patched = find.includes("openFindInPage") && search.includes("refreshSearch") && zdtp.startsWith("throw new Error(");
  if (scope.patches.length > 0 && !patched) errors.push(`${scopeKey} installed zudo-doc is missing the controlled Find/Search/zdtp patch`);
  if (scope.patches.length === 0 && (find.includes("openFindInPage") || search.includes("refreshSearch"))) {
    errors.push(`${scopeKey} installed zudo-doc must be the unpatched upstream package`);
  }
  return errors;
}
