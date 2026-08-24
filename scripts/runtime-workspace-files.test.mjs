import assert from "node:assert/strict";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, test } from "node:test";
import {
  RUNTIME_APP_FILES,
  assertAllowlistedInventory,
  assertRuntimeRenderedPrivacy,
  assertRuntimeWorkspacePrivacy,
  copyRuntimeApp,
} from "./runtime-workspace-files.mjs";

const roots = [];

afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "ccresdoc-runtime-files-"));
  roots.push(root);
  const source = join(root, "source");
  const destination = join(root, "destination");
  for (const relativePath of RUNTIME_APP_FILES) {
    const path = join(source, relativePath);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, relativePath === "zfb.config.ts" ? "export default { plugins: [] };\n" : `${relativePath}\n`);
  }
  return { source, destination };
}

test("runtime source copy admits only generic landings and release-owned files", () => {
  const { source, destination } = fixture();
  mkdirSync(join(source, "src/content/docs/claude-skills"), { recursive: true });
  writeFileSync(join(source, "src/content/docs/claude-skills/private.mdx"), "ccresdoc-private-resource\n");
  mkdirSync(join(source, "src/content/docs/.ccresdoc-resource-transitions"), { recursive: true });
  writeFileSync(join(source, "src/content/docs/.ccresdoc-resource-transitions/state.json"), "private\n");

  const inventory = copyRuntimeApp(source, destination);
  assert.equal(inventory.copiedDistFiles.length, 0);
  assertAllowlistedInventory(destination);
  assert.equal(readFileSync(join(destination, "src/content/docs/claude/index.mdx"), "utf8"), "src/content/docs/claude/index.mdx\n");
  assert.equal(readdirSync(join(destination, "src/content/docs"), { withFileTypes: true }).some((entry) => entry.name === "claude-skills"), false);
  assert.equal(readdirSync(join(destination, "src/content/docs"), { withFileTypes: true }).some((entry) => entry.name.startsWith(".ccresdoc")), false);

  mkdirSync(join(destination, "public"), { recursive: true });
  writeFileSync(join(destination, "public/private.html"), "ccresdoc-private-resource\n");
  assert.throws(() => assertAllowlistedInventory(destination), /unexpected public input/);
});

test("dist route allowlist rejects generated detail routes", () => {
  const { source, destination } = fixture();
  mkdirSync(join(source, "dist/docs/claude"), { recursive: true });
  writeFileSync(join(source, "dist/docs/claude/index.html"), "generic Claude landing\n");
  mkdirSync(join(source, "dist/assets"), { recursive: true });
  writeFileSync(join(source, "dist/assets/app.js"), "generic package asset\n");
  const inventory = copyRuntimeApp(source, destination, { includeDist: true });
  assert.deepEqual(inventory.copiedDistFiles, ["assets/app.js", "docs/claude/index.html"]);
  assert.equal(readFileSync(join(destination, "dist/docs/claude/index.html"), "utf8"), "generic Claude landing\n");

  mkdirSync(join(source, "dist/docs/claude-skills"), { recursive: true });
  writeFileSync(join(source, "dist/docs/claude-skills/private.html"), "ccresdoc-private-resource\n");
  assert.throws(
    () => copyRuntimeApp(source, destination, { includeDist: true }),
    /runtime dist path is not allowlisted: docs\/claude-skills\/private\.html/,
  );
});

test("privacy audit rejects generated paths, fixture content, and checkout paths", () => {
  const { source, destination } = fixture();
  copyRuntimeApp(source, destination);
  const generated = join(destination, "src/content/docs/codex-config/private.mdx");
  mkdirSync(dirname(generated), { recursive: true });
  writeFileSync(generated, "ccresdoc-private-source\n");
  assert.throws(() => assertRuntimeWorkspacePrivacy(destination), /generated resource path was staged/);

  rmSync(generated);
  writeFileSync(join(destination, "src/content/docs/index.mdx"), "/private/synthetic/configured-root\n");
  assert.throws(
    () => assertRuntimeWorkspacePrivacy(destination, { forbiddenPaths: ["/private/synthetic/configured-root"] }),
    /configured checkout path was staged/,
  );
  assert.throws(
    () => assertRuntimeRenderedPrivacy("fixture", "ccresdoc-private-resource"),
    /fixture rendered response leaked fixture sentinel/,
  );
});
