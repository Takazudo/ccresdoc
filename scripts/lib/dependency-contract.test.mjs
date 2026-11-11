import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync, chmodSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { checkInstalled, checkScope, checkScopesAgree, parseWorkspace } from "./dependency-contract.mjs";
import { ZFB_VERSION, ZUDO_DOC_PATCH } from "./toolchain-contract.mjs";

const repoRoot = dirname(dirname(dirname(fileURLToPath(import.meta.url))));
const load = (dir) => {
  const root = join(repoRoot, dir);
  return {
    root,
    packageJson: JSON.parse(readFileSync(join(root, "package.json"), "utf8")),
    lockfile: readFileSync(join(root, "pnpm-lock.yaml"), "utf8"),
    workspace: readFileSync(join(root, "pnpm-workspace.yaml"), "utf8"),
  };
};
const app = load("app");
const probe = load("compatibility/node-free-latest");
const patchBytes = readFileSync(join(app.root, ZUDO_DOC_PATCH.path));

function appInputs(overrides = {}) {
  return {
    packageJson: structuredClone(app.packageJson),
    lockfile: app.lockfile,
    workspace: app.workspace,
    readPatch: (path) => (path === ZUDO_DOC_PATCH.path ? patchBytes : null),
    ...overrides,
  };
}
const expectError = (errors, pattern) => {
  assert.ok(errors.some((error) => pattern.test(error)), `expected ${pattern} in:\n${errors.join("\n")}`);
};

test("the committed app and probe scopes satisfy the audited contract", () => {
  assert.deepEqual(checkScope("app", appInputs()), []);
  assert.deepEqual(checkScope("probe", { ...probe, readPatch: () => null }), []);
  assert.deepEqual(checkScopesAgree(app.packageJson, probe.packageJson), []);
});

test("a mismatched ZFB family pin fails in the manifest", () => {
  const packageJson = structuredClone(app.packageJson);
  packageJson.dependencies["@takazudo/zfb-runtime"] = "6.0.0";
  expectError(checkScope("app", appInputs({ packageJson })), /dependencies\.@takazudo\/zfb-runtime/);
});

test("a mismatched carrier pin fails even when every other pin agrees", () => {
  const packageJson = structuredClone(app.packageJson);
  packageJson.optionalDependencies["@takazudo/zfb-darwin-arm64"] = "6.0.0";
  expectError(checkScope("app", appInputs({ packageJson })), /optionalDependencies\.@takazudo\/zfb-darwin-arm64/);
});

test("a missing carrier declaration fails", () => {
  const packageJson = structuredClone(app.packageJson);
  delete packageJson.optionalDependencies["@takazudo/zfb-win32-x64-msvc"];
  expectError(checkScope("app", appInputs({ packageJson })), /zfb-win32-x64-msvc/);
});

test("a lockfile that resolves a different family version fails", () => {
  const lockfile = app.lockfile.replaceAll(`'@takazudo/zfb-md-wasm@${ZFB_VERSION}'`, "'@takazudo/zfb-md-wasm@6.0.0'");
  expectError(checkScope("app", appInputs({ lockfile })), /lockfile package versions for @takazudo\/zfb-md-wasm/);
});

test("an older Node engine fails", () => {
  const packageJson = structuredClone(app.packageJson);
  packageJson.engines.node = ">=22.0.0";
  expectError(checkScope("app", appInputs({ packageJson })), /engines\.node/);
});

test("a reintroduced Preact or Tailwind dependency fails", () => {
  const packageJson = structuredClone(app.packageJson);
  packageJson.devDependencies.tailwindcss = "4.2.0";
  packageJson.dependencies.preact = "10.29.1";
  const errors = checkScope("app", appInputs({ packageJson }));
  expectError(errors, /forbidden devDependencies entry: tailwindcss/);
  expectError(errors, /forbidden dependencies entry: preact/);
});

test("a changed patch file fails its hash", () => {
  const readPatch = () => Buffer.concat([patchBytes, Buffer.from("\n")]);
  expectError(checkScope("app", appInputs({ readPatch })), /sha256/);
});

test("a missing patch file fails", () => {
  expectError(checkScope("app", appInputs({ readPatch: () => null })), /patch is missing/);
});

test("an unregistered or extra patch fails", () => {
  const unregistered = app.workspace.replace(/^patchedDependencies:\n(?:  .*\n?)+/m, "");
  expectError(checkScope("app", appInputs({ workspace: unregistered })), /workspace patchedDependencies/);
  const extra = `${app.workspace.trimEnd()}\n  '@takazudo/zfb-runtime@${ZFB_VERSION}': patches/runtime.patch\n`;
  expectError(checkScope("app", appInputs({ workspace: extra })), /workspace patchedDependencies/);
});

test("a widened release-age exclusion list fails", () => {
  const workspace = app.workspace.replace(
    "minimumReleaseAgeExclude:\n",
    `minimumReleaseAgeExclude:\n  - '@takazudo/zfb@${ZFB_VERSION}'\n`,
  );
  assert.deepEqual(parseWorkspace(workspace).exclude.length, 2);
  expectError(checkScope("app", appInputs({ workspace })), /minimumReleaseAgeExclude/);
});

test("a patched probe fails because the probe is an unpatched upstream consumer", () => {
  expectError(checkScope("probe", { ...probe, workspace: app.workspace, readPatch: () => null }), /workspace patchedDependencies/);
});

test("app and probe disagreement fails the cross-scope check", () => {
  const probePackage = structuredClone(probe.packageJson);
  probePackage.dependencies.zod = "4.6.6";
  expectError(checkScopesAgree(app.packageJson, probePackage), /dependencies disagree/);
});

test("an installed tree without the host native carrier fails", () => {
  const root = mkdtempSync(join(tmpdir(), "ccresdoc-dep-contract-"));
  try {
    const write = (name, version, extra = {}) => {
      const dir = join(root, "node_modules", ...name.split("/"));
      mkdirSync(dir, { recursive: true });
      writeFileSync(join(dir, "package.json"), JSON.stringify({ name, version, ...extra }));
      return dir;
    };
    for (const [name, version] of Object.entries(probe.packageJson.dependencies)) write(name, version);
    for (const [name, version] of Object.entries(probe.packageJson.devDependencies)) write(name, version);
    const errors = checkInstalled("probe", root, { platform: "linux", arch: "x64" });
    expectError(errors, /native zfb binary is missing/);

    const carrier = write("@takazudo/zfb-linux-x64-gnu", ZFB_VERSION);
    writeFileSync(join(carrier, "zfb"), "");
    chmodSync(join(carrier, "zfb"), 0o644);
    expectError(checkInstalled("probe", root, { platform: "linux", arch: "x64" }), /not executable/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
