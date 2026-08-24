import assert from "node:assert/strict";
import { cpSync, mkdtempSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join } from "node:path";
import { spawnSync } from "node:child_process";
import { resolveNativeBinary, probeRoot } from "./native-binary.mjs";

const variants = ["wholesale", "routes-off", "selected", "manual"];
const results = {};
const escapeRegExp = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
const escapedProbeRoot = escapeRegExp(probeRoot);
const escapedRelativeProbeRoot = escapeRegExp(probeRoot.replace(/^\/+/, ""));
const escapedTempRoot = escapeRegExp(tmpdir());
const normalize = (value) => value
  .replace(new RegExp(`(?:\\.\\.\\/)+${escapedRelativeProbeRoot}`, "g"), "<probe-root>")
  .replace(new RegExp(escapedProbeRoot, "g"), "<probe-root>")
  .replace(
    new RegExp(`(?:/private)?${escapedTempRoot}/ccresdoc-config-[^/ ]+/workspace`, "g"),
    "<isolated-workspace>",
  )
  .replace(
    new RegExp(`(?:/private)?${escapedTempRoot}/zfb-plugin-host-[^/ ]+`, "g"),
    "<zfb-plugin-host>",
  )
  .replace(/ in \d+\.\d+s/g, " in <duration>");
const shellQuote = (value) => `'${value.replaceAll("'", `'"'"'`)}'`;

for (const variant of variants) {
  const stateDir = mkdtempSync(join(tmpdir(), `ccresdoc-config-${variant}-`));
  const workspace = join(stateDir, "workspace");
  const sentinelDir = join(stateDir, "sentinel-bin");
  const sentinelLog = join(stateDir, "node-invocations.log");
  mkdirSync(sentinelDir);
  writeFileSync(sentinelLog, "");
  cpSync(probeRoot, workspace, {
    recursive: true,
    filter: (source) => !["node_modules", "dist", ".zfb", ".zfb-build"].includes(source.split(/[\\/]/).at(-1)),
  });
  symlinkSync(join(probeRoot, "node_modules"), join(workspace, "node_modules"), "dir");
  writeFileSync(join(workspace, "zfb.config.ts"), [
    'import { defineConfig } from "zfb/config";',
    `import config from "./configs/${variant}.mjs";`,
    "export default defineConfig(config);",
    "",
  ].join("\n"));

  const check = spawnSync(resolveNativeBinary(), ["check"], {
    cwd: workspace,
    encoding: "utf8",
  });

  const realNode = process.execPath;
  const sentinel = process.platform === "win32" ? join(sentinelDir, "node.cmd") : join(sentinelDir, "node");
  writeFileSync(sentinel, process.platform === "win32"
    ? `@echo %* >> "${sentinelLog}"\r\n@"${realNode}" %*\r\n`
    : `#!/bin/sh\nprintf '%s\\n' "$*" >> ${shellQuote(sentinelLog)}\nexec ${shellQuote(realNode)} "$@"\n`,
    { mode: 0o755 });
  const build = spawnSync(resolveNativeBinary(), ["build"], {
    cwd: workspace,
    encoding: "utf8",
    env: { ...process.env, PATH: `${sentinelDir}${delimiter}${process.env.PATH ?? ""}` },
  });
  const invocations = readFileSync(sentinelLog, "utf8")
    .split("\n")
    .filter(Boolean);

  results[variant] = {
    checkStatus: check.status,
    buildStatus: build.status,
    nodeInvocationsDuringBuild: invocations.length,
    nodeInvocationArgs: invocations.map(normalize),
    checkTail: `${check.stdout}${check.stderr}`.trim().split("\n").slice(-8).map(normalize),
    buildTail: `${build.stdout}${build.stderr}`.trim().split("\n").slice(-12).map(normalize),
  };
  rmSync(stateDir, { recursive: true, force: true });
}

console.log(JSON.stringify(results, null, 2));

assert.equal(results.selected.checkStatus, 0);
assert.equal(results.selected.buildStatus, 0);
assert.equal(results.selected.nodeInvocationsDuringBuild, 0);
assert.equal(results.manual.checkStatus, 0);
assert.equal(results.manual.buildStatus, 0);
assert.equal(results.manual.nodeInvocationsDuringBuild, 0);
assert.equal(results["routes-off"].checkStatus, 0);
assert.equal(results["routes-off"].buildStatus, 0);
assert.ok(results["routes-off"].nodeInvocationsDuringBuild > 0);
assert.equal(results.wholesale.checkStatus, 0);
assert.equal(results.wholesale.buildStatus, 1);
assert.ok(results.wholesale.nodeInvocationsDuringBuild > 0);
