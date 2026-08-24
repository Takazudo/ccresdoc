import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, test } from "node:test";
import {
  APPLICATION_VERSION_FILES,
  artifactNames,
  normalizeTag,
  readReleaseContract,
  updateReleaseVersion,
} from "./release-contract.mjs";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const helperPath = resolve(repositoryRoot, "scripts", "release-contract.mjs");
const fixtureRoots = [];

const fixtureContents = {
  tauriConfig: `{
  "productName": "CCResDoc",
  "version": "0.1.0",
  "identifier": "com.example.fixture"
}
`,
  cargoManifest: `[package]
name = "ccresdoc"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = "1"
`,
  cargoLock: `# generated fixture
version = 4

[[package]]
name = "ccresdoc"
version = "0.1.0"

[[package]]
name = "serde"
version = "1.0.0"
`,
  packageJson: `{
  "name": "tooling-fixture",
  "version": "0.0.1"
}
`,
  untouched: "keep this file unchanged\n",
};

function makeFixture() {
  const root = mkdtempSync(join(tmpdir(), "release-contract-"));
  fixtureRoots.push(root);
  mkdirSync(join(root, "src-tauri"));
  writeFileSync(join(root, APPLICATION_VERSION_FILES.tauriConfig), fixtureContents.tauriConfig);
  writeFileSync(join(root, APPLICATION_VERSION_FILES.cargoManifest), fixtureContents.cargoManifest);
  writeFileSync(join(root, APPLICATION_VERSION_FILES.cargoLock), fixtureContents.cargoLock);
  writeFileSync(join(root, "package.json"), fixtureContents.packageJson);
  writeFileSync(join(root, "untouched.txt"), fixtureContents.untouched);
  return root;
}

function fixtureFile(root, relativePath) {
  return join(root, relativePath);
}

function snapshotFiles(root, paths = [
  ...Object.values(APPLICATION_VERSION_FILES),
  "package.json",
  "untouched.txt",
]) {
  return new Map(paths.map((path) => [path, readFileSync(fixtureFile(root, path), "utf8")]));
}

function changedFiles(before, root) {
  return [...before.keys()].filter((path) => before.get(path) !== readFileSync(fixtureFile(root, path), "utf8"));
}

function assertContract(contract, version = "0.1.0") {
  const artifactName = `CCResDoc_${version}_aarch64.dmg`;
  assert.deepEqual(contract, {
    version,
    tag: `v${version}`,
    artifactName,
    checksumName: `${artifactName}.sha256`,
    artifactDirectory: "release-artifacts/",
    artifactPath: `release-artifacts/${artifactName}`,
    checksumPath: `release-artifacts/${artifactName}.sha256`,
  });
}

afterEach(() => {
  for (const root of fixtureRoots.splice(0)) rmSync(root, { recursive: true, force: true });
});

test("the real repository exposes a synchronized stable contract read-only", () => {
  const before = snapshotFiles(repositoryRoot, [
    ...Object.values(APPLICATION_VERSION_FILES),
    "package.json",
  ]);
  const contract = readReleaseContract({ rootDir: repositoryRoot });
  assert.match(contract.version, /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/);
  assertContract(contract, contract.version);
  assert.deepEqual(changedFiles(before, repositoryRoot), []);
});

test("a fixture exposes the exact tag, artifact names, and JSON CLI contract", () => {
  const root = makeFixture();
  assertContract(readReleaseContract({ rootDir: root }));
  assert.deepEqual(artifactNames("0.1.0"), {
    artifactName: "CCResDoc_0.1.0_aarch64.dmg",
    checksumName: "CCResDoc_0.1.0_aarch64.dmg.sha256",
  });

  const result = spawnSync(process.execPath, [helperPath, "check", "--root", root, "--json"], {
    encoding: "utf8",
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stderr, "");
  assertContract(JSON.parse(result.stdout));
});

test("fixture version updates change only the three application-version locations", () => {
  const root = makeFixture();
  const before = snapshotFiles(root);
  assertContract(updateReleaseVersion("1.2.3", { rootDir: root }), "1.2.3");
  assert.deepEqual(changedFiles(before, root).sort(), Object.values(APPLICATION_VERSION_FILES).sort());
  assert.equal(readFileSync(fixtureFile(root, "package.json"), "utf8"), before.get("package.json"));
  assert.equal(readFileSync(fixtureFile(root, "untouched.txt"), "utf8"), before.get("untouched.txt"));
});

test("root tooling metadata is ignored by synchronization and updates", () => {
  const root = makeFixture();
  const packagePath = fixtureFile(root, "package.json");
  writeFileSync(packagePath, readFileSync(packagePath, "utf8").replace("0.0.1", "9.9.9"));
  assertContract(readReleaseContract({ rootDir: root }));
  updateReleaseVersion("1.0.0", { rootDir: root });
  assert.match(readFileSync(packagePath, "utf8"), /"version": "9\.9\.9"/);
});

test("stable semver and tag validation rejects non-stable or malformed values", () => {
  for (const value of [
    "",
    "1",
    "1.2",
    "1.2.3-alpha",
    "1.2.3+build",
    "01.2.3",
    "v1.2.3",
  ]) {
    assert.throws(() => updateReleaseVersion(value, { rootDir: makeFixture() }), /stable semver/);
  }
  assert.equal(normalizeTag("1.2.3"), "v1.2.3");
  assert.equal(normalizeTag("v1.2.3"), "v1.2.3");
  for (const value of ["V1.2.3", "vv1.2.3", "v1.2", "v1.2.3-alpha", "v1.2.3+build"]) {
    assert.throws(() => normalizeTag(value), /stable semver|tag must/);
  }
});

test("each authority mismatch is rejected", () => {
  for (const [relativePath, replacement] of [
    [APPLICATION_VERSION_FILES.tauriConfig, ["0.1.0", "0.1.1"]],
    [APPLICATION_VERSION_FILES.cargoManifest, ["version = \"0.1.0\"", "version = \"0.1.1\""]],
    [APPLICATION_VERSION_FILES.cargoLock, ["version = \"0.1.0\"", "version = \"0.1.1\""]],
  ]) {
    const root = makeFixture();
    const path = fixtureFile(root, relativePath);
    writeFileSync(path, readFileSync(path, "utf8").replace(replacement[0], replacement[1]));
    assert.throws(() => readReleaseContract({ rootDir: root }), /version mismatch/);
  }
});

test("missing and duplicate ccresdoc lock entries are rejected", () => {
  const missingRoot = makeFixture();
  const missingPath = fixtureFile(missingRoot, APPLICATION_VERSION_FILES.cargoLock);
  writeFileSync(
    missingPath,
    readFileSync(missingPath, "utf8").replace(
      /\n\[\[package\]\]\nname = "ccresdoc"\nversion = "0\.1\.0"\n/,
      "\n",
    ),
  );
  assert.throws(() => readReleaseContract({ rootDir: missingRoot }), /missing ccresdoc package entry/);

  const duplicateRoot = makeFixture();
  const duplicatePath = fixtureFile(duplicateRoot, APPLICATION_VERSION_FILES.cargoLock);
  writeFileSync(
    duplicatePath,
    `${readFileSync(duplicatePath, "utf8")}\n[[package]]\nname = "ccresdoc"\nversion = "0.1.0"\n`,
  );
  assert.throws(() => readReleaseContract({ rootDir: duplicateRoot }), /duplicate ccresdoc package entries/);
});

test("malformed authority files fail with concise diagnostics and non-zero CLI status", () => {
  const root = makeFixture();
  writeFileSync(fixtureFile(root, APPLICATION_VERSION_FILES.tauriConfig), "{\n  \"version\":\n");
  assert.throws(() => readReleaseContract({ rootDir: root }), /malformed src-tauri\/tauri\.conf\.json/);

  const result = spawnSync(process.execPath, [helperPath, "normalize-tag", "v1.2.3-alpha"], {
    encoding: "utf8",
  });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /^release-contract: /);
  assert.equal(result.stdout, "");
});

test("duplicate manifest version keys fail rather than selecting an ambiguous match", () => {
  const root = makeFixture();
  const path = fixtureFile(root, APPLICATION_VERSION_FILES.cargoManifest);
  writeFileSync(path, readFileSync(path, "utf8").replace(
    'version = "0.1.0"\nedition',
    'version = "0.1.0"\nversion = "0.1.0"\nedition',
  ));
  assert.throws(() => readReleaseContract({ rootDir: root }), /duplicate version/);
});

test("the documented JSON command can update a fixture through the root override", () => {
  const root = makeFixture();
  const result = spawnSync(process.execPath, [
    helperPath,
    "set-version",
    "2.0.0",
    "--root",
    root,
    "--json",
  ], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stderr, "");
  assertContract(JSON.parse(result.stdout), "2.0.0");
});
