import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { validatePublicationSnapshot } from "./release-publication.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const targetSha = "0123456789abcdef0123456789abcdef01234567";
const tagObjectSha = "abcdef0123456789abcdef0123456789abcdef01";
const artifactName = "CCResDoc_0.1.0_aarch64.dmg";
const checksumName = `${artifactName}.sha256`;

function fixture() {
  return {
    tag: "v0.1.0",
    targetSha,
    mainSha: targetSha,
    source: { version: "0.1.0", tag: "v0.1.0", artifactName, checksumName },
    remoteTag: { kind: "absent", objectSha: null, peeledSha: null },
    ci: {
      databaseId: 41,
      workflowPath: ".github/workflows/ci.yml",
      workflowName: "CI",
      event: "push",
      branch: "main",
      headSha: targetSha,
      status: "completed",
      conclusion: "success",
    },
    release: {
      databaseId: 73,
      tagName: "v0.1.0",
      targetCommitish: targetSha,
      draft: true,
      prerelease: false,
      latest: false,
      assets: [{ id: 101, name: artifactName }, { id: 102, name: checksumName }],
    },
    checksum: { text: `${"a".repeat(64)}  ${artifactName}\n`, verified: true },
  };
}

function rejected(mutator, pattern) {
  const value = fixture();
  mutator(value);
  assert.throws(() => validatePublicationSnapshot(value), pattern);
}

test("matching draft with an absent tag validates and returns mutation-safe IDs", () => {
  assert.deepEqual(validatePublicationSnapshot(fixture()), {
    disposition: "draft",
    releaseDatabaseId: 73,
    artifactName,
    checksumName,
    assetIds: { [artifactName]: 101, [checksumName]: 102 },
  });
});

test("strict tag and SHA inputs plus source agreement fail closed", () => {
  for (const tag of ["0.1.0", "v01.1.0", "v1.2", "v1.2.3-rc.1", "v1.2.3+build"]) {
    rejected((value) => { value.tag = tag; }, /strict v<stable-semver>|does not match source/);
  }
  rejected((value) => { value.targetSha = targetSha.toUpperCase(); }, /full lowercase 40-hex SHA/);
  rejected((value) => { value.targetSha = targetSha.slice(1); }, /full lowercase 40-hex SHA/);
  rejected((value) => { value.source.version = "0.1.1"; }, /source\.tag must equal/);
  rejected((value) => { value.source.artifactName = "wrong.dmg"; }, /source\.artifactName/);
});

test("absent, exact lightweight, and exact peeled annotated tags are accepted", () => {
  const lightweight = fixture();
  lightweight.remoteTag = { kind: "lightweight", objectSha: targetSha, peeledSha: targetSha };
  assert.equal(validatePublicationSnapshot(lightweight).disposition, "draft");
  const annotated = fixture();
  annotated.remoteTag = { kind: "annotated", objectSha: tagObjectSha, peeledSha: targetSha };
  assert.equal(validatePublicationSnapshot(annotated).disposition, "draft");
  rejected((value) => {
    value.remoteTag = { kind: "annotated", objectSha: tagObjectSha, peeledSha: "1".repeat(40) };
  }, /remote tag peels/);
  rejected((value) => {
    value.remoteTag = { kind: "absent", objectSha: targetSha, peeledSha: null };
  }, /absent remote tag/);
  rejected((value) => {
    value.remoteTag = { kind: "annotated", objectSha: targetSha, peeledSha: targetSha };
  }, /annotated tag object/);
});

test("main and every required CI attribute must match", () => {
  rejected((value) => { value.mainSha = "1".repeat(40); }, /current main/);
  for (const [field, replacement, pattern] of [
    ["workflowPath", ".github/workflows/other.yml", /workflow path/],
    ["workflowName", "Other", /workflow name/],
    ["event", "workflow_dispatch", /event=push/],
    ["branch", "topic", /branch=main/],
    ["headSha", "1".repeat(40), /head SHA/],
    ["status", "in_progress", /completed/],
    ["conclusion", "failure", /successful conclusion/],
  ]) rejected((value) => { value.ci[field] = replacement; }, pattern);
});

test("draft identity, target, state, and exact inventory are enforced", () => {
  rejected((value) => { value.release.tagName = "v0.1.1"; }, /release tag/);
  rejected((value) => { value.release.targetCommitish = "main"; }, /release target/);
  rejected((value) => { value.release.prerelease = true; }, /cannot be a prerelease/);
  rejected((value) => { value.release.assets.pop(); }, /release assets must be exactly/);
  rejected((value) => { value.release.assets.push({ id: 103, name: "extra.zip" }); }, /release assets must be exactly/);
  rejected((value) => { value.release.assets[1].name = artifactName; }, /asset names must be unique/);
});

test("checksum syntax, basename, lowercase digest, newline, and verification are strict", () => {
  rejected((value) => { value.checksum.text = `${"A".repeat(64)}  ${artifactName}\n`; }, /lowercase GNU/);
  rejected((value) => { value.checksum.text = `${"a".repeat(64)} *${artifactName}\n`; }, /lowercase GNU/);
  rejected((value) => { value.checksum.text = `${"a".repeat(64)}  wrong.dmg\n`; }, /must name/);
  rejected((value) => { value.checksum.text = `${"a".repeat(64)}  ${artifactName}`; }, /ending in a newline/);
  rejected((value) => { value.checksum.verified = false; }, /evidence is false/);
});

test("publication recheck requires the same draft database and asset IDs", () => {
  const expected = {
    releaseDatabaseId: 73,
    assetIds: { [artifactName]: 101, [checksumName]: 102 },
  };
  assert.equal(validatePublicationSnapshot(fixture(), { phase: "prepublish", expected }).disposition, "draft");
  assert.throws(() => validatePublicationSnapshot({ ...fixture(), release: { ...fixture().release, databaseId: 74 } }, {
    phase: "prepublish", expected,
  }), /database ID/);
  const replaced = fixture();
  replaced.release.assets[0].id = 999;
  assert.throws(() => validatePublicationSnapshot(replaced, { phase: "prepublish", expected }), /asset.*database ID/);
  const raced = fixture();
  raced.release.draft = false;
  raced.remoteTag = { kind: "lightweight", objectSha: targetSha, peeledSha: targetSha };
  assert.throws(() => validatePublicationSnapshot(raced, { phase: "prepublish", expected }), /became published/);
});

test("published retry is complete only with exact release, peeled tag, target, and assets", () => {
  const value = fixture();
  value.release.draft = false;
  value.release.latest = true;
  value.remoteTag = { kind: "annotated", objectSha: tagObjectSha, peeledSha: targetSha };
  assert.equal(validatePublicationSnapshot(value).disposition, "already_complete");
  assert.throws(() => validatePublicationSnapshot({ ...value, remoteTag: { kind: "absent", objectSha: null, peeledSha: null } }), /published release requires/);
  assert.throws(() => validatePublicationSnapshot({ ...value, release: { ...value.release, targetCommitish: "main" } }), /release target/);
  assert.throws(() => validatePublicationSnapshot({ ...value, release: { ...value.release, assets: value.release.assets.slice(0, 1) } }), /release assets/);
  assert.throws(() => validatePublicationSnapshot({ ...value, release: { ...value.release, latest: false } }), /must be Latest/);
  assert.equal(validatePublicationSnapshot(value, { phase: "published" }).disposition, "already_complete");
});

test("workflow structure is Ubuntu-only, pinned, permission-split, and guarded", () => {
  const workflow = readFileSync(resolve(root, ".github/workflows/release.yml"), "utf8");
  assert.match(workflow, /run-name: Release \$\{\{ inputs\.tag \}\} @ \$\{\{ inputs\.target_sha \}\} \[\$\{\{ inputs\.request_id \}\}\]/);
  assert.match(workflow, /validation_only:\n\s+description:.*\n\s+required: true\n\s+type: boolean\n\s+default: true/);
  assert.match(workflow, /cancel-in-progress: false/);
  assert.equal((workflow.match(/runs-on: ubuntu-latest/g) ?? []).length, 2);
  assert.doesNotMatch(workflow, /macos-|cargo tauri build|xcodebuild|hdiutil/);
  assert.match(workflow, /validate:[\s\S]*?permissions:\n\s+actions: read\n\s+contents: read/);
  assert.match(workflow, /publish:[\s\S]*?if:.*inputs\.validation_only == false[\s\S]*?permissions:\n\s+actions: read\n\s+contents: write/);
  assert.equal((workflow.match(/actions\/workflows\/ci\.yml\/runs/g) ?? []).length, 3);
  assert.equal((workflow.match(/--method PATCH/g) ?? []).length, 1);
  assert.ok(workflow.indexOf("publish:") < workflow.indexOf("--method PATCH"));
  for (const line of workflow.split("\n").filter((entry) => /uses:/.test(entry))) {
    assert.match(line, /@[0-9a-f]{40}\s+#\s+v\d/);
  }
});

test("CLI rejects invalid JSON snapshots without printing successful output", () => {
  const result = spawnSync(process.execPath, [resolve(root, "scripts/release-publication.mjs"), "validate", "/dev/null"], { encoding: "utf8" });
  assert.equal(result.status, 1);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, /^release-publication:/);
});
