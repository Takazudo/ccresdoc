import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { createReadStream, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { compile } from "@takazudo/zfb-md-wasm";
import { checkInstalled } from "../../../scripts/lib/dependency-contract.mjs";
import {
  NATIVE_CARRIERS,
  RELEASE_AGE_EXCLUDES,
  ZFB_VERSION,
} from "../../../scripts/lib/toolchain-contract.mjs";
import { probeRoot, resolveNativeBinary } from "./native-binary.mjs";

const platformPackages = Object.values(NATIVE_CARRIERS);

assert.deepEqual(checkInstalled("probe", probeRoot), [], "installed probe tree matches the audited contract");

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
  assert.ok(lockfile.includes(`'${platformPackage}@${ZFB_VERSION}':`), `${platformPackage} locked package`);
}

const workspace = readFileSync(join(probeRoot, "pnpm-workspace.yaml"), "utf8");
for (const exclusion of RELEASE_AGE_EXCLUDES) {
  assert.ok(workspace.includes(`  - "${exclusion}"`), `${exclusion} release-age exclusion`);
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
