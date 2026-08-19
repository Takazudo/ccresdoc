import assert from "node:assert/strict";
import { readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { probeRoot, resolveNativeBinary } from "./native-binary.mjs";

const expected = {
  "@takazudo/zfb": "2.7.1",
  "@takazudo/zfb-md-wasm": "2.7.1",
  "@takazudo/zfb-runtime": "2.7.1",
  "@takazudo/zudo-doc": "5.6.0",
  katex: "0.16.22",
  preact: "10.29.1",
  "preact-render-to-string": "6.6.7",
  zod: "4.3.6",
};

for (const [name, version] of Object.entries(expected)) {
  const manifestPath = join(probeRoot, "node_modules", ...name.split("/"), "package.json");
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  assert.equal(manifest.version, version, `${name} installed version`);
}

const lockfile = readFileSync(join(probeRoot, "pnpm-lock.yaml"), "utf8");
for (const platformPackage of [
  "@takazudo/zfb-darwin-arm64",
  "@takazudo/zfb-darwin-x64",
  "@takazudo/zfb-linux-arm64-gnu",
  "@takazudo/zfb-linux-x64-gnu",
  "@takazudo/zfb-win32-x64-msvc",
]) {
  assert.ok(lockfile.includes(`'${platformPackage}':`), `${platformPackage} importer pin`);
  assert.ok(lockfile.includes(`'${platformPackage}@2.7.1':`), `${platformPackage} locked package`);
}

const binaryMode = statSync(resolveNativeBinary()).mode;
if (process.platform !== "win32") assert.notEqual(binaryMode & 0o111, 0, "native binary executable mode");

console.log("package and native-binary assertions passed");
