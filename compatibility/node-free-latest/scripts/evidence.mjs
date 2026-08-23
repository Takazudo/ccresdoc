import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { collectPackageFacts } from "./package-facts.mjs";
import { probeRoot } from "./native-binary.mjs";

const mode = process.argv[2] ?? "--check";
if (!["--check", "--refresh"].includes(mode)) {
  throw new Error("usage: evidence.mjs [--check|--refresh]");
}

function runJsonScript(script) {
  const result = spawnSync(process.execPath, [join(probeRoot, "scripts", script)], {
    cwd: probeRoot,
    encoding: "utf8",
    env: process.env,
    maxBuffer: 40 * 1024 * 1024,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${script} failed (${result.status})\n${result.stdout}${result.stderr}`);
  }
  try {
    return JSON.parse(result.stdout);
  } catch (error) {
    throw new Error(`${script} did not emit one JSON document\n${result.stdout}${result.stderr}`, { cause: error });
  }
}

const generated = {
  "package-facts.json": await collectPackageFacts(),
  "resolved-configs.json": runJsonScript("dump-configs.mjs"),
  "config-matrix.json": runJsonScript("probe-matrix.mjs"),
  "native-runtime.json": runJsonScript("probe-native-runtime.mjs"),
};

const changed = [];
for (const [name, value] of Object.entries(generated)) {
  const path = join(probeRoot, "evidence", name);
  const normalized = `${JSON.stringify(value, null, 2)}\n`;
  if (mode === "--refresh") {
    writeFileSync(path, normalized);
  } else {
    const committed = readFileSync(path, "utf8");
    if (committed !== normalized) changed.push(name);
  }
}

if (mode === "--check") {
  assert.deepEqual(changed, [], `compatibility evidence drifted: ${changed.join(", ")}`);
  console.log("compatibility evidence matches observed package and command outputs");
} else {
  console.log("compatibility evidence refreshed from observed package and command outputs");
}
