import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { artifactNames, readReleaseContract } from "./release-contract.mjs";
import { validatePublicationSnapshot } from "./release-publication.mjs";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const workflowPath = resolve(repositoryRoot, ".github/workflows/release.yml");

function source(relativePath) {
  return readFileSync(resolve(repositoryRoot, relativePath), "utf8");
}

const contract = readReleaseContract({ rootDir: repositoryRoot });
const packageManifest = JSON.parse(source("package.json"));
const readme = source("README.md");
const skill = source(".claude/skills/l-make-release/SKILL.md");
const workflow = source(".github/workflows/release.yml");
const ciWorkflow = source(".github/workflows/ci.yml");
const producer = source("scripts/build-macos-release.sh");
const packageProbe = source("scripts/test-macos-package.sh");
const publication = source("scripts/release-publication.mjs");

function assertNoMachineSpecificPaths(label, value) {
  const slash = String.raw`/`;
  const machinePathPatterns = [
    new RegExp(`(?:^|[\\s"'\\x60])(?:${slash}(?:Users|home|private${slash}var${slash}folders)${slash})`),
    /(?:^|[\s"'`])[A-Za-z]:[\\/]/,
    new RegExp(`(?:^|[\\s"'\\x60])${slash}${slash}(?:Users|home)${slash}${slash}`),
  ];
  for (const pattern of machinePathPatterns) {
    assert.doesNotMatch(value, pattern, `${label} contains a machine-specific absolute path`);
  }
}

function publicationFixture() {
  const targetSha = "0123456789abcdef0123456789abcdef01234567";
  const tagObjectSha = "abcdef0123456789abcdef0123456789abcdef01";
  const assetIds = { [contract.artifactName]: 101, [contract.checksumName]: 102 };
  return {
    tag: contract.tag,
    targetSha,
    mainSha: targetSha,
    source: {
      version: contract.version,
      tag: contract.tag,
      artifactName: contract.artifactName,
      checksumName: contract.checksumName,
    },
    remoteTag: { kind: "annotated", objectSha: tagObjectSha, peeledSha: targetSha },
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
      tagName: contract.tag,
      targetCommitish: targetSha,
      draft: true,
      prerelease: false,
      latest: false,
      assets: Object.entries(assetIds).map(([name, id]) => ({ id, name })),
    },
    checksum: { text: `${"a".repeat(64)}  ${contract.artifactName}\n`, verified: true },
  };
}

test("the current version contract crosses the publication boundary unchanged", () => {
  assert.match(contract.version, /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/);
  assert.deepEqual(artifactNames(contract.version), {
    artifactName: contract.artifactName,
    checksumName: contract.checksumName,
  });
  assert.match(contract.artifactName, /^CCResDoc_(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)_aarch64\.dmg$/);
  assert.equal(contract.checksumName, `${contract.artifactName}.sha256`);
  assert.equal(contract.artifactPath, `release-artifacts/${contract.artifactName}`);
  assert.equal(contract.checksumPath, `release-artifacts/${contract.checksumName}`);

  const result = validatePublicationSnapshot(publicationFixture());
  assert.deepEqual(result, {
    disposition: "draft",
    releaseDatabaseId: 73,
    artifactName: contract.artifactName,
    checksumName: contract.checksumName,
    assetIds: { [contract.artifactName]: 101, [contract.checksumName]: 102 },
  });
  assert.match(publication, /import \{ artifactNames, normalizeTag \} from "\.\/release-contract\.mjs"/);
  assert.match(publication, /source\.artifactName/);
  assert.match(publication, /source\.checksumName/);
});

test("README and release skill expose the same artifact, verification, and signing contract", () => {
  for (const document of [readme, skill]) {
    assert.match(document, /CCResDoc_<(?:version|semver)>_aarch64\.dmg/);
    assert.match(document, /CCResDoc_<(?:version|semver)>_aarch64\.dmg\.sha256/);
    assert.doesNotMatch(document, /CCResDoc_[^\s`<>]+_x86_64\.dmg/);
  }

  assert.match(readme, /shasum -a 256 -c CCResDoc_<version>_aarch64\.dmg\.sha256/);
  assert.match(readme, /bash scripts\/build-macos-release\.sh/);
  assert.match(readme, /without uploading|no upload/i);
  assert.match(readme, /pnpm rebuild:local-app/);
  assert.match(readme, /scripts\/test-macos-package\.sh/);
  assert.match(readme, /Apple Silicon \(`aarch64`\)/);
  assert.match(readme, /ad-hoc signed but not notarized/);
  assert.match(readme, /right-click `CCResDoc\.app` and choose \*\*Open\*\*/);
  assert.match(readme, /System Settings → Privacy & Security/);

  assert.match(skill, /\/l-make-release\n\/l-make-release major\n\/l-make-release minor\n\/l-make-release patch/);
  assert.match(skill, /\/l-make-release --confirm \[major\|minor\|patch\]/);
  assert.match(skill, /\/l-make-release cancel/);
  assert.match(skill, /bash scripts\/build-macos-release\.sh --upload "\$tag"/);
  assert.match(skill, /--clobber/);
  assert.match(skill, /Do not dispatch `\.github\/workflows\/release\.yml` in this mode\./);
  assert.match(skill, /-f validation_only=false/);
  assert.match(skill, /Release <tag> @ <target_sha> \[<request_id>\]/);
  assert.match(skill, /Interrupted release-bump recovery/);
  assert.match(skill, /exactly one commit after the latest stable tag/);
  assert.match(skill, /never infer another component and bump twice/);
  assert.match(skill, /when its asset inventory is empty,[\s\S]*remove only the two contract-derived local paths/);
  assert.doesNotMatch(skill, /--method DELETE "repos\/\$repo\/git\/refs\/tags/);
  assert.match(skill, /Do not delete a remote tag automatically/);
});

test("the producer derives its pair from the contract and keeps upload mutation opt-in", () => {
  assert.match(producer, /Usage:\n  bash scripts\/build-macos-release\.sh\n  bash scripts\/build-macos-release\.sh --upload v<stable-semver> \[--clobber\]/);
  assert.match(producer, /CONTRACT_JSON=.*release-contract\.mjs" check --root "\$REPO_ROOT" --json/);
  assert.match(producer, /ARTIFACT_PATH="\$REPO_ROOT\/\$ARTIFACT_RELATIVE_PATH"/);
  assert.match(producer, /CHECKSUM_PATH="\$REPO_ROOT\/\$CHECKSUM_RELATIVE_PATH"/);
  assert.match(producer, /BUILT_DMG="\$TARGET_DIR\/release\/bundle\/dmg\/\$ARTIFACT_NAME"/);
  assert.match(producer, /APPLE_SIGNING_IDENTITY=- cargo tauri build --bundles dmg/);
  assert.doesNotMatch(producer, /codesign[^\n]+\|\s*grep\s+-q/);
  assert.match(producer, /grep -q '\^Signature=adhoc\$' <<<"\$SIGNATURE_DETAILS"/);
  assert.match(producer, /hdiutil verify "\$BUILT_DMG"/);
  assert.match(producer, /hdiutil attach -readonly -nobrowse -mountpoint/);
  assert.match(producer, /bash "\$SCRIPT_DIR\/test-macos-package\.sh" --existing-bundle "\$APP_PATH"/);
  assert.match(producer, /shasum -a 256 -c "\$CHECKSUM_NAME"/);
  assert.match(producer, /if \[\[ -n "\$UPLOAD_TAG" \]\]/);
  assert.match(producer, /assert_upload_source "before build"/);
  assert.match(producer, /assert_upload_source "immediately before upload"/);
  assert.match(producer, /upload requires the main branch/);
  assert.match(producer, /upload requires a clean working tree/);
  assert.match(producer, /quit the existing CCResDoc instance before building/);
  assert.match(producer, /gh release upload/);
  assert.match(producer, /echo "upload=disabled"/);
  assert.match(producer, /--clobber is allowed only with --upload/);
  assert.match(producer, /const names=\(r\.assets \?\? \[\]\)\.map\(\(a\) => a\.name\)\.sort\(\)/);
  assert.doesNotMatch(producer, /filter\(\(name\) => \/\^CCResDoc_/);
  assert.doesNotMatch(producer, /gh release create|gh release publish|gh workflow run/);
  assert.match(packageProbe, /Refusing to run beside an existing CCResDoc instance/);
  assert.match(packageProbe, /tell application id "com\.takazudo\.ccresdoc" to quit/);
  assert.match(packageProbe, /Packaged CCResDoc did not quit during probe cleanup/);
});

test("the publication workflow is parseable, input-correlated, permission-split, and Ubuntu-only", () => {
  const yamlCheck = spawnSync("ruby", ["-e", "require 'yaml'; YAML.load_file(ARGV.fetch(0))", workflowPath], {
    encoding: "utf8",
  });
  if (!yamlCheck.error || yamlCheck.error.code !== "ENOENT") {
    assert.equal(yamlCheck.status, 0, yamlCheck.stderr);
  }

  assert.match(workflow, /^name: Release$/m);
  assert.match(workflow, /^run-name: Release \$\{\{ inputs\.tag \}\} @ \$\{\{ inputs\.target_sha \}\} \[\$\{\{ inputs\.request_id \}\}\]$/m);
  assert.match(workflow, /workflow_dispatch:\n\s+inputs:/);
  for (const input of ["tag", "target_sha", "request_id"]) {
    assert.match(workflow, new RegExp(`${input}:\\n\\s+description:[^\\n]+\\n\\s+required: true\\n\\s+type: string`));
  }
  assert.match(workflow, /validation_only:\n\s+description:[^\n]+\n\s+required: true\n\s+type: boolean\n\s+default: true/);
  assert.match(workflow, /concurrency:\n\s+group: release-\$\{\{ inputs\.tag \}\}\n\s+cancel-in-progress: false/);

  const validateStart = workflow.indexOf("  validate:");
  const publishStart = workflow.indexOf("  publish:");
  assert.ok(validateStart >= 0 && publishStart > validateStart);
  const validateJob = workflow.slice(validateStart, publishStart);
  const publishJob = workflow.slice(publishStart);
  assert.match(validateJob, /runs-on: ubuntu-latest/);
  assert.match(validateJob, /permissions:\n\s+actions: read\n\s+contents: read/);
  assert.match(publishJob, /if: \$\{\{ inputs\.validation_only == false && needs\.validate\.outputs\.disposition == 'draft' \}\}/);
  assert.match(publishJob, /runs-on: ubuntu-latest/);
  assert.match(publishJob, /permissions:\n\s+actions: read\n\s+contents: write/);
  assert.doesNotMatch(workflow, /(?:runs-on:\s*macos-|cargo tauri build|xcodebuild|hdiutil)/i);
  assert.equal((workflow.match(/--method PATCH/g) ?? []).length, 1);
  assert.equal((workflow.match(/actions\/workflows\/ci\.yml\/runs/g) ?? []).length, 3);
  for (const line of workflow.split("\n").filter((entry) => /\buses:/.test(entry))) {
    assert.match(line, /@[0-9a-f]{40}\s+#\s+v\d/);
  }
});

test("release-owned text contains no machine-specific paths", () => {
  const releaseOwnedFiles = [
    "README.md",
    ".claude/skills/l-make-release/SKILL.md",
    ".github/workflows/release.yml",
    "scripts/build-macos-release.sh",
    "scripts/release-contract.mjs",
    "scripts/release-publication.mjs",
    "package.json",
  ];
  for (const relativePath of releaseOwnedFiles) {
    const value = source(relativePath);
    assertNoMachineSpecificPaths(relativePath, value);
  }
});

test("package metadata exposes the complete deterministic release-flow test", () => {
  assert.equal(packageManifest.scripts["test:release-flow"], "node --test scripts/release-flow-integration.test.mjs");
  assert.match(
    ciWorkflow,
    /pnpm run test:release-contract && pnpm run test:release-publication && pnpm run test:release-flow/,
  );
});
