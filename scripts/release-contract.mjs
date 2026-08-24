#!/usr/bin/env node

import {
  readFileSync,
  writeFileSync,
} from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));

export const DEFAULT_ROOT_DIR = resolve(scriptDirectory, "..");
export const APPLICATION_VERSION_FILES = Object.freeze({
  tauriConfig: "src-tauri/tauri.conf.json",
  cargoManifest: "src-tauri/Cargo.toml",
  cargoLock: "Cargo.lock",
});
export const ARTIFACT_DIRECTORY = "release-artifacts/";
export const STABLE_SEMVER_PATTERN = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

const ARTIFACT_DIRECTORY_NAME = ARTIFACT_DIRECTORY.slice(0, -1);
const SOURCE_LABELS = Object.freeze({
  tauriConfig: APPLICATION_VERSION_FILES.tauriConfig,
  cargoManifest: APPLICATION_VERSION_FILES.cargoManifest,
  cargoLock: APPLICATION_VERSION_FILES.cargoLock,
});

export class ReleaseContractError extends Error {
  constructor(message, { exitCode = 1 } = {}) {
    super(message);
    this.name = "ReleaseContractError";
    this.exitCode = exitCode;
  }
}

class UsageError extends ReleaseContractError {
  constructor(message) {
    super(message, { exitCode: 2 });
    this.name = "UsageError";
  }
}

function contractError(message) {
  return new ReleaseContractError(message);
}

function usageError(message) {
  return new UsageError(message);
}

function rootDirectory(options) {
  if (typeof options === "string") return resolve(options);
  if (options == null) return DEFAULT_ROOT_DIR;
  if (typeof options !== "object") {
    throw contractError("root override must be a directory path");
  }
  const root = options.rootDir ?? options.root ?? DEFAULT_ROOT_DIR;
  if (typeof root !== "string" || root.length === 0) {
    throw contractError("root override must be a directory path");
  }
  return resolve(root);
}

function readSource(root, relativePath) {
  try {
    return readFileSync(resolve(root, relativePath), "utf8");
  } catch (error) {
    if (error?.code === "ENOENT") {
      throw contractError(`missing source file: ${relativePath}`);
    }
    throw contractError(`cannot read source file: ${relativePath}`);
  }
}

function stableSemver(value) {
  return typeof value === "string" && STABLE_SEMVER_PATTERN.test(value);
}

export function isStableSemver(value) {
  return stableSemver(value);
}

export function validateStableSemver(value, label = "version") {
  if (!stableSemver(value)) {
    throw contractError(`${label} must be a stable semver (MAJOR.MINOR.PATCH)`);
  }
  return value;
}

export function normalizeTag(value) {
  if (typeof value !== "string" || value.length === 0) {
    throw contractError("tag must be v<stable-semver> or <stable-semver>");
  }
  const version = value.startsWith("v") ? value.slice(1) : value;
  validateStableSemver(version, "tag");
  return `v${version}`;
}

export const normalizeReleaseTag = normalizeTag;

export function artifactNames(version) {
  validateStableSemver(version);
  const artifactName = `CCResDoc_${version}_aarch64.dmg`;
  return {
    artifactName,
    checksumName: `${artifactName}.sha256`,
  };
}

export const getArtifactNames = artifactNames;

function contractFromVersion(version) {
  validateStableSemver(version);
  const tag = normalizeTag(version);
  const { artifactName, checksumName } = artifactNames(version);
  return {
    version,
    tag,
    artifactName,
    checksumName,
    artifactDirectory: ARTIFACT_DIRECTORY,
    artifactPath: `${ARTIFACT_DIRECTORY_NAME}/${artifactName}`,
    checksumPath: `${ARTIFACT_DIRECTORY_NAME}/${checksumName}`,
  };
}

function skipJsonWhitespace(source, start) {
  let index = start;
  while (index < source.length && /\s/.test(source[index])) index += 1;
  return index;
}

function jsonStringEnd(source, start, label) {
  if (source[start] !== '"') throw contractError(`malformed ${label}: invalid JSON`);
  for (let index = start + 1; index < source.length; index += 1) {
    const character = source[index];
    if (character === "\\") {
      index += 1;
      continue;
    }
    if (character === '"') return index + 1;
    if (character < " ") throw contractError(`malformed ${label}: invalid JSON`);
  }
  throw contractError(`malformed ${label}: invalid JSON`);
}

function jsonValueEnd(source, start, label) {
  const first = source[start];
  if (first === '"') return jsonStringEnd(source, start, label);
  if (first === "{" || first === "[") {
    const stack = [first === "{" ? "}" : "]"];
    let index = start + 1;
    while (index < source.length && stack.length > 0) {
      const character = source[index];
      if (character === '"') {
        index = jsonStringEnd(source, index, label);
        continue;
      }
      if (character === "{" || character === "[") {
        stack.push(character === "{" ? "}" : "]");
      } else if (character === "}" || character === "]") {
        if (stack.at(-1) !== character) {
          throw contractError(`malformed ${label}: invalid JSON`);
        }
        stack.pop();
        if (stack.length === 0) return index + 1;
      }
      index += 1;
    }
    throw contractError(`malformed ${label}: invalid JSON`);
  }

  let index = start;
  while (index < source.length && source[index] !== "," && source[index] !== "}") index += 1;
  let end = index;
  while (end > start && /\s/.test(source[end - 1])) end -= 1;
  return end;
}

function scanTopLevelJsonProperties(source, label) {
  let index = skipJsonWhitespace(source, 0);
  if (source[index] !== "{") throw contractError(`malformed ${label}: top-level value must be an object`);
  index = skipJsonWhitespace(source, index + 1);
  const properties = [];
  const seen = new Set();
  if (source[index] === "}") return properties;

  while (index < source.length) {
    const keyStart = index;
    const keyEnd = jsonStringEnd(source, keyStart, label);
    let key;
    try {
      key = JSON.parse(source.slice(keyStart, keyEnd));
    } catch {
      throw contractError(`malformed ${label}: invalid JSON`);
    }
    if (seen.has(key)) throw contractError(`malformed ${label}: duplicate top-level key: ${key}`);
    seen.add(key);

    index = skipJsonWhitespace(source, keyEnd);
    if (source[index] !== ":") throw contractError(`malformed ${label}: invalid JSON`);
    const valueStart = skipJsonWhitespace(source, index + 1);
    const valueEnd = jsonValueEnd(source, valueStart, label);
    properties.push({ key, valueStart, valueEnd });

    index = skipJsonWhitespace(source, valueEnd);
    if (source[index] === "}") return properties;
    if (source[index] !== ",") throw contractError(`malformed ${label}: invalid JSON`);
    index = skipJsonWhitespace(source, index + 1);
    if (source[index] === "}") throw contractError(`malformed ${label}: invalid JSON`);
  }
  throw contractError(`malformed ${label}: invalid JSON`);
}

function parseTauriConfig(source) {
  const label = SOURCE_LABELS.tauriConfig;
  let parsed;
  try {
    parsed = JSON.parse(source);
  } catch {
    throw contractError(`malformed ${label}: invalid JSON`);
  }
  if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw contractError(`malformed ${label}: top-level value must be an object`);
  }
  const properties = scanTopLevelJsonProperties(source, label);
  const versionProperties = properties.filter(({ key }) => key === "version");
  if (versionProperties.length === 0) throw contractError(`missing version in ${label}`);
  if (versionProperties.length !== 1) throw contractError(`duplicate version in ${label}`);
  if (typeof parsed.version !== "string") throw contractError(`malformed version in ${label}`);
  const { valueStart, valueEnd } = versionProperties[0];
  return {
    version: parsed.version,
    source,
    versionStart: valueStart,
    versionEnd: valueEnd,
  };
}

function tomlCommentStart(line) {
  let quote = null;
  for (let index = 0; index < line.length; index += 1) {
    const character = line[index];
    if (quote === '"') {
      if (character === "\\") {
        index += 1;
      } else if (character === '"') {
        quote = null;
      }
    } else if (quote === "'") {
      if (character === "'") quote = null;
    } else if (character === '"' || character === "'") {
      quote = character;
    } else if (character === "#") {
      return index;
    }
  }
  return line.length;
}

function sourceLines(source) {
  const lines = [];
  const pattern = /([^\r\n]*)(\r\n|\n|$)/g;
  for (const match of source.matchAll(pattern)) {
    lines.push({ line: match[1], offset: match.index });
    if (match[2] === "") break;
  }
  return lines;
}

function parseQuotedTomlString(raw, label, field) {
  const value = raw.trim();
  if (value.length < 2 || !((value.startsWith('"') && value.endsWith('"'))
    || (value.startsWith("'") && value.endsWith("'")))) {
    throw contractError(`malformed ${field} in ${label}`);
  }
  const quote = value[0];
  const inner = value.slice(1, -1);
  if (quote === '"' && /(?:^|[^\\])(?:\\\\)*\\[^"'\\]/.test(inner)) {
    throw contractError(`malformed ${field} in ${label}`);
  }
  return quote === '"' ? inner.replace(/\\(["\\])/g, "$1") : inner;
}

function parseCargoManifest(source) {
  const label = SOURCE_LABELS.cargoManifest;
  let currentTable = null;
  let packageHeaders = 0;
  let versionEntry = null;

  for (const { line, offset } of sourceLines(source)) {
    const code = line.slice(0, tomlCommentStart(line)).trim();
    if (code.length === 0) continue;

    const arrayTable = /^\[\[([^\]]+)\]\]$/.exec(code);
    if (arrayTable) {
      currentTable = null;
      continue;
    }
    const table = /^\[([^\]]+)\]$/.exec(code);
    if (table) {
      currentTable = table[1].trim();
      if (currentTable === "package") {
        packageHeaders += 1;
        if (packageHeaders > 1) throw contractError(`duplicate [package] table in ${label}`);
      }
      continue;
    }
    if (code.startsWith("[")) throw contractError(`malformed table in ${label}`);
    if (currentTable !== "package") continue;

    const assignment = /^([A-Za-z0-9_-]+)\s*=\s*(.*)$/.exec(code);
    if (!assignment || assignment[1] !== "version") continue;
    if (versionEntry) throw contractError(`duplicate version in ${label}`);

    const value = assignment[2].trim();
    const parsed = parseQuotedTomlString(value, label, "version");
    const codeStart = line.indexOf(code);
    const valueStartInCode = assignment[0].indexOf(assignment[2]) + assignment[2].indexOf(value);
    const valueStart = offset + codeStart + valueStartInCode;
    versionEntry = {
      version: parsed,
      source,
      versionStart: valueStart,
      versionEnd: valueStart + value.length,
    };
  }

  if (packageHeaders === 0) throw contractError(`missing [package] table in ${label}`);
  if (!versionEntry) throw contractError(`missing version in ${label}`);
  return versionEntry;
}

function parseCargoLock(source) {
  const label = SOURCE_LABELS.cargoLock;
  const lines = sourceLines(source);
  const headers = [];
  for (const [index, { line }] of lines.entries()) {
    const code = line.slice(0, tomlCommentStart(line)).trim();
    if (code === "[[package]]") headers.push(index);
    else if (code.startsWith("[[")) throw contractError(`malformed package table in ${label}`);
  }
  if (headers.length === 0) throw contractError(`missing [[package]] entries in ${label}`);

  const entries = [];
  for (const [headerIndex, start] of headers.entries()) {
    const end = headers[headerIndex + 1] ?? lines.length;
    let nameEntry = null;
    let versionEntry = null;
    for (let index = start + 1; index < end; index += 1) {
      const { line, offset } = lines[index];
      const code = line.slice(0, tomlCommentStart(line)).trim();
      const assignment = /^([A-Za-z0-9_-]+)\s*=\s*(.*)$/.exec(code);
      if (!assignment || !["name", "version"].includes(assignment[1])) continue;
      const key = assignment[1];
      if (key === "name" && nameEntry) throw contractError(`duplicate name in ${label}`);
      if (key === "version" && versionEntry) throw contractError(`duplicate version in ${label}`);
      const value = parseQuotedTomlString(assignment[2], label, key);
      const codeStart = line.indexOf(code);
      const valueStartInCode = assignment[0].indexOf(assignment[2]) + assignment[2].indexOf(assignment[2].trim());
      const valueStart = offset + codeStart + valueStartInCode;
      const entry = {
        value,
        source,
        versionStart: key === "version" ? valueStart : undefined,
        versionEnd: key === "version" ? valueStart + assignment[2].trim().length : undefined,
      };
      if (key === "name") nameEntry = entry;
      else versionEntry = entry;
    }
    if (!nameEntry) throw contractError(`package entry missing name in ${label}`);
    if (!versionEntry) throw contractError(`package entry missing version in ${label}`);
    entries.push({
      name: nameEntry.value,
      version: versionEntry.value,
      source,
      versionStart: versionEntry.versionStart,
      versionEnd: versionEntry.versionEnd,
    });
  }

  const matches = entries.filter(({ name }) => name === "ccresdoc");
  if (matches.length === 0) throw contractError(`missing ccresdoc package entry in ${label}`);
  if (matches.length !== 1) throw contractError(`duplicate ccresdoc package entries in ${label}`);
  return matches[0];
}

function readParsedSources(root) {
  const tauriConfig = parseTauriConfig(readSource(root, APPLICATION_VERSION_FILES.tauriConfig));
  const cargoManifest = parseCargoManifest(readSource(root, APPLICATION_VERSION_FILES.cargoManifest));
  const cargoLock = parseCargoLock(readSource(root, APPLICATION_VERSION_FILES.cargoLock));
  return { tauriConfig, cargoManifest, cargoLock };
}

function assertSynchronized(sources) {
  const entries = [
    [SOURCE_LABELS.tauriConfig, sources.tauriConfig],
    [SOURCE_LABELS.cargoManifest, sources.cargoManifest],
    [SOURCE_LABELS.cargoLock, sources.cargoLock],
  ];
  const versions = entries.map(([, { version }]) => version);
  for (const [label, { version }] of entries) {
    validateStableSemver(version, label);
  }
  if (!versions.every((version) => version === versions[0])) {
    throw contractError(
      `version mismatch: ${SOURCE_LABELS.tauriConfig}=${versions[0]}; `
      + `${SOURCE_LABELS.cargoManifest}=${versions[1]}; ${SOURCE_LABELS.cargoLock}=${versions[2]}`,
    );
  }
  return versions[0];
}

export function readReleaseContract(options = {}) {
  const root = rootDirectory(options);
  const version = assertSynchronized(readParsedSources(root));
  return contractFromVersion(version);
}

export function updateReleaseVersion(versionOrOptions, options = {}) {
  let version = versionOrOptions;
  let updateOptions = options;
  if (versionOrOptions !== null && typeof versionOrOptions === "object") {
    version = versionOrOptions.version;
    updateOptions = versionOrOptions;
  }
  validateStableSemver(version);
  const root = rootDirectory(updateOptions);
  const sources = readParsedSources(root);
  assertSynchronized(sources);

  const updates = [
    [APPLICATION_VERSION_FILES.tauriConfig, sources.tauriConfig],
    [APPLICATION_VERSION_FILES.cargoManifest, sources.cargoManifest],
    [APPLICATION_VERSION_FILES.cargoLock, sources.cargoLock],
  ];
  for (const [relativePath, source] of updates) {
    if (source.version === version) continue;
    const updated = `${source.source.slice(0, source.versionStart)}${JSON.stringify(version)}${source.source.slice(source.versionEnd)}`;
    writeFileSync(resolve(root, relativePath), updated, "utf8");
  }
  return readReleaseContract({ rootDir: root });
}

export const updateVersion = updateReleaseVersion;
export const readVersionContract = readReleaseContract;

export const USAGE_TEXT = `Usage:
  node scripts/release-contract.mjs check [--root <directory>] [--json]
  node scripts/release-contract.mjs set-version <stable-semver> [--root <directory>] [--json]
  node scripts/release-contract.mjs normalize-tag <tag> [--json]

The check command reads the three synchronized application-version locations and
prints one JSON object. The set-version command updates those locations and then
prints the same object. The JSON fields are version, tag, artifactName,
checksumName, artifactDirectory, artifactPath, and checksumPath. Artifacts belong
under release-artifacts/ and use the exact names CCResDoc_<semver>_aarch64.dmg
and CCResDoc_<semver>_aarch64.dmg.sha256. The helper only validates, names, and
updates the contract; it does not build or copy release files.

Exit codes:
  0  success
  1  invalid arguments, malformed/missing/ambiguous source state, or I/O failure
  2  command usage error
`;

function parseCliArguments(argumentsList) {
  const positional = [];
  let root;
  let help = false;
  for (let index = 0; index < argumentsList.length; index += 1) {
    const argument = argumentsList[index];
    if (argument === "--help" || argument === "-h") {
      help = true;
    } else if (argument === "--json") {
      // JSON is the stable output format; accepting this flag makes the
      // machine-readable invocation explicit for scripts and documentation.
    } else if (argument === "--root") {
      root = argumentsList[index + 1];
      if (!root || root.startsWith("-")) throw usageError("--root requires a directory path");
      index += 1;
    } else if (argument.startsWith("--root=")) {
      root = argument.slice("--root=".length);
      if (!root) throw usageError("--root requires a directory path");
    } else if (argument.startsWith("-")) {
      throw usageError(`unknown option: ${argument}`);
    } else {
      positional.push(argument);
    }
  }
  return { positional, root, help };
}

function cliContractForTag(tag) {
  const normalized = normalizeTag(tag);
  return contractFromVersion(normalized.slice(1));
}

export function runCli(argumentsList = process.argv.slice(2)) {
  try {
    const { positional, root, help } = parseCliArguments(argumentsList);
    if (help) {
      process.stdout.write(USAGE_TEXT);
      return 0;
    }
    const command = positional.shift() ?? "check";
    let contract;
    if (command === "check" || command === "read") {
      if (positional.length > 0) throw usageError(`${command} does not accept positional arguments`);
      contract = readReleaseContract({ rootDir: root });
    } else if (command === "set-version" || command === "update") {
      if (positional.length !== 1) throw usageError(`${command} requires one stable semver`);
      contract = updateReleaseVersion(positional[0], { rootDir: root });
    } else if (command === "normalize-tag" || command === "tag") {
      if (positional.length !== 1) throw usageError(`${command} requires one tag`);
      contract = cliContractForTag(positional[0]);
    } else {
      throw usageError(`unknown command: ${command}`);
    }
    process.stdout.write(`${JSON.stringify(contract)}\n`);
    return 0;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    process.stderr.write(`release-contract: ${message}\n`);
    return error?.exitCode === 2 ? 2 : 1;
  }
}

const isMainModule = process.argv[1]
  && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url));
if (isMainModule) process.exitCode = runCli();
