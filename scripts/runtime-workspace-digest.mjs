import { createHash } from "node:crypto";
import {
  lstatSync,
  readFileSync,
  readlinkSync,
  readdirSync,
} from "node:fs";
import { join, relative, sep } from "node:path";

export const RUNTIME_WORKSPACE_DIGEST_ALGORITHM = "sha256-tree-v1";

function portablePath(path) {
  return path.split(sep).join("/");
}

function updateEntry(hash, root, path) {
  const stat = lstatSync(path);
  const label = portablePath(relative(root, path));
  const mode = (stat.mode & 0o777).toString(8);

  if (stat.isDirectory()) {
    hash.update("directory\0").update(label).update("\0").update(mode).update("\0");
    for (const name of readdirSync(path).sort()) {
      updateEntry(hash, root, join(path, name));
    }
    return;
  }
  if (stat.isSymbolicLink()) {
    hash.update("symlink\0").update(label).update("\0").update(mode).update("\0")
      .update(readlinkSync(path)).update("\0");
    return;
  }
  if (stat.isFile()) {
    hash.update("file\0").update(label).update("\0").update(mode).update("\0")
      .update(String(stat.size)).update("\0").update(readFileSync(path)).update("\0");
    return;
  }
  throw new Error(`runtime workspace digest: unsupported entry ${path}`);
}

export function runtimeWorkspaceDigest(root, { implementationFiles = [] } = {}) {
  const hash = createHash("sha256");
  hash.update(`${RUNTIME_WORKSPACE_DIGEST_ALGORITHM}\0`);
  for (const name of readdirSync(root).sort()) {
    updateEntry(hash, root, join(root, name));
  }
  for (const { label, path } of [...implementationFiles]
    .sort((left, right) => (left.label < right.label ? -1 : left.label > right.label ? 1 : 0))) {
    const stat = lstatSync(path);
    if (!stat.isFile()) {
      throw new Error(`runtime workspace digest: implementation is not a file: ${path}`);
    }
    hash.update("implementation\0").update(label).update("\0")
      .update(String(stat.size)).update("\0").update(readFileSync(path)).update("\0");
  }
  return hash.digest("hex");
}

export function refreshTokenFromWorkspaceDigest(digest) {
  return createHash("sha256")
    .update("ccresdoc-runtime-workspace-v1\0")
    .update(digest)
    .digest("hex")
    .slice(0, 32);
}
