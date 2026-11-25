import { createHash } from "node:crypto";
import { readFileSync, realpathSync } from "node:fs";
import { join, isAbsolute } from "node:path";
import {
  RUNTIME_WORKSPACE_DIGEST_ALGORITHM,
  refreshTokenFromWorkspaceDigest,
  runtimeWorkspaceDigest,
} from "./runtime-workspace-digest.mjs";

const [bundle, revision] = process.argv.slice(2);
if (!bundle || !isAbsolute(bundle) || !/^[a-f0-9]{40}$/.test(revision ?? "")) {
  throw new Error("Expected an absolute .app path and full source SHA");
}

const bundlePath = realpathSync(bundle);
const contents = join(bundlePath, "Contents");
const runtime = join(contents, "Resources/runtime-workspace");
const manifestBytes = readFileSync(join(runtime, "runtime-manifest.json"));
const manifest = JSON.parse(manifestBytes);
const refreshToken = readFileSync(join(runtime, "version.txt"), "utf8").trim();
const nativeBinary = "app/node_modules/@takazudo/zfb-darwin-arm64/zfb";
if (manifest.refreshToken !== refreshToken ||
    manifest.workspaceDigest?.algorithm !== RUNTIME_WORKSPACE_DIGEST_ALGORITHM ||
    manifest.nativeBinary !== nativeBinary ||
    !/^[a-f0-9]{64}$/.test(manifest.workspaceDigest?.value ?? "") ||
    refreshTokenFromWorkspaceDigest(manifest.workspaceDigest.value) !== refreshToken) {
  throw new Error("Invalid packaged runtime manifest or refresh token");
}
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
console.log(JSON.stringify({
  revision,
  bundlePath,
  contentsSha256TreeV1: runtimeWorkspaceDigest(contents),
  hostSha256: sha256(readFileSync(join(contents, "MacOS/ccresdoc"))),
  plistSha256: sha256(readFileSync(join(contents, "Info.plist"))),
  sidecarSha256: sha256(readFileSync(join(runtime, nativeBinary))),
  manifestSha256: sha256(manifestBytes),
  refreshToken,
  workspaceDigest: manifest.workspaceDigest,
}, null, 2));
