import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const packageByPlatform = {
  "darwin-arm64": "@takazudo/zfb-darwin-arm64",
  "darwin-x64": "@takazudo/zfb-darwin-x64",
  "linux-arm64": "@takazudo/zfb-linux-arm64-gnu",
  "linux-x64": "@takazudo/zfb-linux-x64-gnu",
  "win32-x64": "@takazudo/zfb-win32-x64-msvc",
};

export function resolveNativeBinary() {
  const packageName = packageByPlatform[`${process.platform}-${process.arch}`];
  if (!packageName) throw new Error(`unsupported probe platform: ${process.platform}-${process.arch}`);
  const packageJson = require.resolve(`${packageName}/package.json`);
  const binary = join(dirname(packageJson), process.platform === "win32" ? "zfb.exe" : "zfb");
  if (!existsSync(binary)) throw new Error(`native zfb binary missing: ${binary}`);
  return binary;
}

export const probeRoot = dirname(dirname(fileURLToPath(import.meta.url)));
