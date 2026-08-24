#!/usr/bin/env node

import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  assertAllowlistedInventory,
  assertRuntimeWorkspacePrivacy,
} from "./runtime-workspace-files.mjs";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const target = resolve(process.argv[2] ?? join(repoRoot, "src-tauri/runtime-workspace/app"));
assert(existsSync(target), `runtime workspace does not exist: ${target}`);
assertAllowlistedInventory(target);
const audit = assertRuntimeWorkspacePrivacy(target, {
  // Callers may pass synthetic checkout/config paths after the target. They
  // are checked as text only and never copied into the artifact.
  forbiddenPaths: process.argv.slice(3),
});

const manifestPath = join(dirname(target), "runtime-manifest.json");
if (existsSync(manifestPath)) {
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  assert.equal(manifest.privacy?.audit, "staged-app-surfaces");
  assert.equal(manifest.privacy?.filesChecked, audit.filesChecked);
  assert.equal(manifest.privacy?.sentinelsChecked, audit.sentinelsChecked);
}

console.log(JSON.stringify({ status: "passed", target, ...audit }, null, 2));
