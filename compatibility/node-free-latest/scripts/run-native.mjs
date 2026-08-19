import { spawnSync } from "node:child_process";
import { resolveNativeBinary, probeRoot } from "./native-binary.mjs";

const args = process.argv.slice(2);
if (args.length === 0) throw new Error("usage: run-native.mjs <zfb args...>");

const result = spawnSync(resolveNativeBinary(), args, {
  cwd: probeRoot,
  env: process.env,
  stdio: "inherit",
});

if (result.error) throw result.error;
process.exit(result.status ?? 1);
