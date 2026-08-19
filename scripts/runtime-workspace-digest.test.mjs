import assert from "node:assert/strict";
import {
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, test } from "node:test";
import {
  refreshTokenFromWorkspaceDigest,
  runtimeWorkspaceDigest,
} from "./runtime-workspace-digest.mjs";

const roots = [];

afterEach(() => {
  for (const root of roots.splice(0)) {
    rmSync(root, { recursive: true, force: true });
  }
});

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "ccresdoc-runtime-digest-"));
  roots.push(root);
  const workspace = join(root, "workspace");
  const implementation = join(root, "stage-runtime-workspace.mjs");
  mkdirSync(join(workspace, "pages", "lib"), { recursive: true });
  writeFileSync(join(workspace, "pages", "lib", "_chrome.ts"), "export const chrome = 1;\n");
  writeFileSync(join(workspace, "zfb.config.ts"), "export default { plugins: [] };\n");
  writeFileSync(implementation, "stage version one\n");
  return {
    workspace,
    implementation,
    options: {
      implementationFiles: [
        { label: "scripts/stage-runtime-workspace.mjs", path: implementation },
      ],
    },
  };
}

test("digest is stable for identical staged bytes", () => {
  const data = fixture();
  const first = runtimeWorkspaceDigest(data.workspace, data.options);
  const second = runtimeWorkspaceDigest(data.workspace, data.options);
  assert.equal(first, second);
  assert.match(refreshTokenFromWorkspaceDigest(first), /^[a-f0-9]{32}$/);
});

test("a staged page change produces a different refresh token", () => {
  const data = fixture();
  const before = refreshTokenFromWorkspaceDigest(
    runtimeWorkspaceDigest(data.workspace, data.options),
  );
  writeFileSync(
    join(data.workspace, "pages", "lib", "_chrome.ts"),
    "export const chrome = 2;\n",
  );
  const after = refreshTokenFromWorkspaceDigest(
    runtimeWorkspaceDigest(data.workspace, data.options),
  );
  assert.notEqual(after, before);
});

test("staging implementation changes also invalidate the workspace", () => {
  const data = fixture();
  const before = runtimeWorkspaceDigest(data.workspace, data.options);
  writeFileSync(data.implementation, "stage version two\n");
  assert.notEqual(runtimeWorkspaceDigest(data.workspace, data.options), before);
});
