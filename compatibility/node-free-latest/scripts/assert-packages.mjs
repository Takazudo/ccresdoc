import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { createReadStream, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { compile } from "@takazudo/zfb-md-wasm";
import { probeRoot, resolveNativeBinary } from "./native-binary.mjs";

const expected = {
  "@takazudo/zfb": "2.15.1",
  "@takazudo/zfb-md-wasm": "2.15.1",
  "@takazudo/zfb-runtime": "2.15.1",
  "@takazudo/zudo-doc": "5.17.2",
  katex: "0.16.22",
  preact: "10.29.1",
  "preact-render-to-string": "6.6.7",
  zod: "4.3.6",
};
const platformPackages = [
  "@takazudo/zfb-darwin-arm64",
  "@takazudo/zfb-darwin-x64",
  "@takazudo/zfb-linux-arm64-gnu",
  "@takazudo/zfb-linux-x64-gnu",
  "@takazudo/zfb-win32-x64-msvc",
];

for (const [name, version] of Object.entries(expected)) {
  const manifestPath = join(probeRoot, "node_modules", ...name.split("/"), "package.json");
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  assert.equal(manifest.version, version, `${name} installed version`);
}

const compilerProbe = await compile("# Root compiler entrypoint probe");
assert.match(compilerProbe.code, /Root compiler entrypoint probe/);
assert.deepEqual(compilerProbe.diagnostics, []);

const mdWasmManifest = JSON.parse(readFileSync(
  join(probeRoot, "node_modules", "@takazudo", "zfb-md-wasm", "package.json"),
  "utf8",
));
assert.ok(mdWasmManifest.exports["./render"], "focused render entrypoint is exported");
assert.ok(mdWasmManifest.exports["./parse"], "focused parse entrypoint is exported");

const lockfile = readFileSync(join(probeRoot, "pnpm-lock.yaml"), "utf8");
for (const platformPackage of platformPackages) {
  assert.ok(lockfile.includes(`'${platformPackage}':`), `${platformPackage} importer pin`);
  assert.ok(lockfile.includes(`'${platformPackage}@2.15.1':`), `${platformPackage} locked package`);
}

const workspace = readFileSync(join(probeRoot, "pnpm-workspace.yaml"), "utf8");
for (const [name, version] of [
  ...Object.entries(expected).filter(([name]) => name.startsWith("@takazudo/")),
  ...platformPackages.map((name) => [name, "2.15.1"]),
]) {
  assert.ok(
    workspace.includes(`  - "${name}@${version}"`),
    `${name}@${version} release-age exclusion`,
  );
}

const packageFacts = JSON.parse(readFileSync(join(probeRoot, "evidence", "package-facts.json"), "utf8"));
for (const fact of [
  ...Object.values(packageFacts.packages),
  ...Object.values(packageFacts.nativeCarriers),
]) {
  assert.ok(lockfile.includes(fact.integrity), `${fact.integrity} present in lockfile`);
}

const platformKey = {
  "darwin-arm64": "darwin-arm64",
  "darwin-x64": "darwin-x64",
  "linux-arm64": "linux-arm64-gnu",
  "linux-x64": "linux-x64-gnu",
  "win32-x64": "win32-x64-msvc",
}[`${process.platform}-${process.arch}`];
assert.ok(platformKey, `supported native carrier for ${process.platform}-${process.arch}`);
const hostFact = packageFacts.nativeCarriers[platformKey];
assert.ok(hostFact, `${platformKey} canonical carrier fact`);
const nativeBinary = resolveNativeBinary();
const binaryMode = statSync(nativeBinary).mode;
if (process.platform !== "win32") assert.notEqual(binaryMode & 0o111, 0, "native binary executable mode");
assert.equal(statSync(nativeBinary).size, hostFact.sizeBytes, "installed native binary size");
const binaryHash = createHash("sha256");
for await (const chunk of createReadStream(nativeBinary)) binaryHash.update(chunk);
assert.equal(binaryHash.digest("hex"), hostFact.sha256, "installed native binary hash");

console.log("package and native-binary assertions passed");
