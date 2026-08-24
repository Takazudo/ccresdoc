---
name: l-make-release
description: "Prepare and publish a CCResDoc macOS release through the checked-in release contract, local producer, and publication workflow."
user-invocable: true
---

# Make a CCResDoc release

This is the complete project-local release procedure. It is autonomous by
default: `/l-make-release` prepares and publishes the next stable release after
all gates pass. It never builds or publishes a release while this skill is being
implemented or tested.

Supported invocations are:

```text
/l-make-release
/l-make-release major
/l-make-release minor
/l-make-release patch
/l-make-release --confirm [major|minor|patch]
/l-make-release cancel
```

The optional component forces that stable semver component. With no component,
the level is inferred from commits. `--confirm` is a human gate and a
draft-only mode: show the proposed version and body before any mutation,
continue only after an explicit confirmation, then prepare and verify the
unpublished draft and stop without dispatching the publication workflow. A
declined confirmation makes no repository or GitHub mutation. A later normal
invocation recognizes the exact matching draft and resumes from it; it does not
try to infer a new release from the now-empty tag range.

`cancel` is the only invocation that removes state. It can remove only a
verified unpublished matching draft and its still-unpublished tag. It never
deletes a published Release or tag and never rewrites shared history.

## Authorities and invariants

Use the repository helpers as the authorities. Do not duplicate the build,
asset, checksum, or publication logic in ad-hoc commands:

- `scripts/release-contract.mjs` is the version and artifact-name contract.
  Its synchronized application-version sources are exactly
  `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, and `Cargo.lock`.
- `scripts/build-macos-release.sh --upload <tag>` is the local macOS arm64
  producer and the only supported upload path. It creates the exact pair under
  `release-artifacts/`, verifies the mounted DMG and nested signatures, and
  updates an existing draft only. It never creates or publishes a Release.
- `.github/workflows/ci.yml` is the ordinary push CI gate.
- `.github/workflows/release.yml` is the fail-closed validator/publisher. Its
  dispatch inputs are `tag`, `target_sha`, `request_id`, and
  `validation_only`; its default branch is `main`.
- `scripts/release-publication.mjs` is the publication snapshot validator used
  by the workflow. Do not weaken its exact tag, target, CI, draft, asset, or
  checksum requirements.

Every mutation below has a checked precondition. Stop on a conflict, an
ambiguous API response, a changed SHA, an unexpected asset, or a failed gate.
Never use `git reset`, `git push --force`, `git push --force-with-lease`,
`git commit --amend`, or an unbounded delete. Do not use `gh release delete` on
an object that has not first been proved to be the exact draft being canceled.

## 1. Preflight the release machine and `main`

Run from the repository root in a trusted shell with `set -euo pipefail`.
The producer itself enforces the first two checks, but the skill must fail
before doing any other release work when they are not true:

```bash
test "$(uname -s)" = "Darwin"
test "$(uname -m)" = "arm64"
for tool in cargo pnpm node git gh hdiutil codesign lipo shasum find readlink stat open curl lsof osascript; do
  command -v "$tool" >/dev/null || exit 1
done
test -x /usr/libexec/PlistBuddy
gh auth status
repo="$(gh repo view --json nameWithOwner --jq .nameWithOwner)"
```

Require a clean local `main`, then synchronize it without ever overwriting
local or remote history:

```bash
test -z "$(git status --porcelain=v1)"
test "$(git branch --show-current)" = "main" || git switch main
test -z "$(git status --porcelain=v1)"
git fetch origin --tags --prune
git pull --ff-only origin main
test "$(git rev-parse --abbrev-ref HEAD)" = "main"
test "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)"
test -z "$(git status --porcelain=v1)"
```

Read the synchronized contract before determining a target. The JSON output
is authoritative; do not read a second version source to override it.

```bash
contract_json="$(node scripts/release-contract.mjs check --root . --json)"
version="$(node -e 'process.stdout.write(JSON.parse(process.argv[1]).version)' "$contract_json")"
tag="$(node -e 'process.stdout.write(JSON.parse(process.argv[1]).tag)' "$contract_json")"
artifact_name="$(node -e 'process.stdout.write(JSON.parse(process.argv[1]).artifactName)' "$contract_json")"
checksum_name="$(node -e 'process.stdout.write(JSON.parse(process.argv[1]).checksumName)' "$contract_json")"
artifact_path="$(node -e 'process.stdout.write(JSON.parse(process.argv[1]).artifactPath)' "$contract_json")"
checksum_path="$(node -e 'process.stdout.write(JSON.parse(process.argv[1]).checksumPath)' "$contract_json")"
```

The helper must succeed and report one strict `MAJOR.MINOR.PATCH` version. The
artifact contract must remain exactly:

```text
CCResDoc_<semver>_aarch64.dmg
CCResDoc_<semver>_aarch64.dmg.sha256
release-artifacts/<those exact names>
```

Do not stage or carry an old local pair into a new build. If either exact path
already exists, keep it until the corresponding draft state is inspected; on a
verified retry, remove only those two contract-derived paths, never a glob or
the whole `release-artifacts/` directory.

Before a bump, run the focused tests while the repository is still at its
current version:

```bash
node --test scripts/release-contract.test.mjs scripts/release-publication.test.mjs
```

After any version change, run the contract check again and at least the
publication tests. The contract test contains a real-repository initial-version
assertion, so do not claim that it passes after changing the real version just
because its fixture tests pass.

## 2. Enumerate only stable tags and Releases

Fetch tags before this step. Treat a tag as stable only when its complete name
matches this exact regular expression:

```text
^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$
```

Enumerate remote tag refs with `git ls-remote --tags origin`, strip the
`refs/tags/` prefix and an optional `^{}` peel suffix, deduplicate, and apply
that expression. For each retained tag, preserve the object and peeled SHA
information. Enumerate Releases through the paginated API, filter
`.tag_name` with the same expression, and retain their draft/published state,
target, prerelease flag, latest flag, URL, database ID, and asset names. Do not
use an unfiltered `gh release list` result to choose a version. `_attachments`
and every other non-semver tag or Release are ignored completely, including
when deciding whether this is the first release.

A useful inspection shape is:

```bash
git ls-remote --tags origin "refs/tags/*" "refs/tags/*^{}"
gh api --paginate --slurp "repos/$repo/releases?per_page=100"
```

The first command can return two lines for an annotated tag. Classify the
remote tag as `absent`, `lightweight`, or `annotated`:

- zero matching ref lines: `absent`, with both object and peeled SHA null;
- one line: `lightweight`, where object SHA equals peeled SHA; and
- two lines: `annotated`, with distinct object and peeled SHAs.

Any other shape is ambiguous and stops the release. If a tag exists, its peeled
SHA must equal the intended commit SHA. A tag conflict is never repaired by
deleting or moving the tag.

## 3. Determine the version, range, and notes

First inspect the Release for the current contract tag. An exact draft whose
tag, target, and synchronized source version all agree is a resumable release;
reuse it and skip inference. An exact published Release is handled as an
idempotent completed release only after the final-state checks in section 9.
Any conflicting draft, published Release, duplicate API match, prerelease, or
target mismatch stops.

If no resumable Release exists, use the following cases.

### Initial release

When there is no retained stable tag and no retained stable Release, the
synchronized version must be exactly `0.1.0`. The target is `v0.1.0`, with the
current clean `main` SHA, and **do not call `set-version` or fabricate a bump
commit**. An existing `v0.1.0` tag is allowed only when it peels to that exact
SHA; an absent tag is allowed until draft creation. The first-release notes are
independent of `_attachments`: use the repository's own history from the root
through the target commit, not a prior tag, generated notes, or a Release-list
ordering.

### Later stable release

Choose the highest retained stable semver tag by numeric semver ordering. If a
stable Release exists without its corresponding remote tag, stop as an
inconsistent state. The synchronized current version must equal the latest tag
version before a new bump; otherwise stop rather than guessing which version
is authoritative. Define the non-empty range explicitly as:

```text
<latest-stable-tag>..<current-main-sha>
```

Reject an empty range (`git rev-list --count <range>` must be greater than
zero). Inspect every commit in that range, including its subject and body:

- breaking change: a conventional header with `!` before `:`, or a footer
  containing `BREAKING CHANGE:` or `BREAKING-CHANGE:`;
- feature: a subject beginning `feat:` or `feat(<scope>):`; and
- other: every remaining commit.

Without an explicit component, breaking changes infer `major`; otherwise any
feature infers `minor`; otherwise infer `patch`. An explicit `major`, `minor`,
or `patch` overrides that inference but never bypasses the non-empty-range,
clean-main, CI, or state checks. Calculate the next stable version with
integer semver arithmetic: major is `(M+1).0.0`, minor is `M.(m+1).0`, and
patch is `M.m.(p+1)`.

Print the proposed current version, target version/tag, target SHA, selected
level and reason, range, and categorized commit list. Build the body in a
temporary file before any version mutation. Use a real file, not escaped inline
newlines; for example, the file should contain headings such as:

```text
## Changes

### Breaking changes
...

### Features
...

### Fixes and other changes
...
```

Create that file in a narrowly scoped temporary location and require it to be
non-empty before continuing:

```bash
body_file="$(mktemp "${TMPDIR:-/tmp}/ccresdoc-release-notes.XXXXXX")"
trap 'rm -f "$body_file"' EXIT
test -s "$body_file"
```

Include each commit once. For the initial release use the root-to-target commit
list. For later releases use only `git log --reverse <latest-stable-tag>..<target
SHA> ...`; the explicit latest stable tag is the range boundary. Preserve the
body file until a draft is created or the run is canceled, and clean up only
that temporary file on exit.

With `--confirm`, show the complete proposal and body now and ask for an
explicit confirmation. Do not mutate version files, Git, Releases, tags, or
assets before that confirmation. If confirmed, continue through draft creation,
local build/upload, and exact draft verification, then stop with the draft
unpublished. Do not dispatch `.github/workflows/release.yml` in this mode.

## 4. Bump only through the shared contract

For a later release, after the proposal is accepted, update all three version
authorities atomically through the helper:

```bash
node scripts/release-contract.mjs set-version "$target_version" --root . --json
node scripts/release-contract.mjs check --root . --json
```

The check must report the proposed version, tag, and exact artifact names. With
the repository initially clean, the only changed paths must be:

```text
Cargo.lock
src-tauri/Cargo.toml
src-tauri/tauri.conf.json
```

Before staging, assert that the clean-start diff contains exactly those three
paths and no untracked release input:

```bash
changed_paths="$(git diff --name-only | sort | awk 'BEGIN { first=1 } { if (!first) printf " "; printf "%s", $0; first=0 }')"
test "$changed_paths" = "Cargo.lock src-tauri/Cargo.toml src-tauri/tauri.conf.json"
test -z "$(git ls-files --others --exclude-standard)"
```

Run `git diff --check` and the focused publication tests. Commit those three
files together with a clear release-bump message, then record the full SHA:

```bash
git add Cargo.lock src-tauri/Cargo.toml src-tauri/tauri.conf.json
git commit -m "chore: release v<target-version>"
target_sha="$(git rev-parse HEAD)"
test -z "$(git status --porcelain=v1)"
test "$target_sha" = "$(printf '%s' "$target_sha" | tr '[:upper:]' '[:lower:]')"
test "$(git diff-tree --no-commit-id --name-only -r HEAD | sort | awk 'BEGIN { first=1 } { if (!first) printf " "; printf "%s", $0; first=0 }')" = "Cargo.lock src-tauri/Cargo.toml src-tauri/tauri.conf.json"
```

Do not include notes, artifacts, or unrelated changes in that commit. Push only
the fast-forward `main` update and verify the remote is exactly the recorded
SHA:

```bash
test "$(git branch --show-current)" = "main"
git push origin main
test "$(git ls-remote origin refs/heads/main | awk '{print $1}')" = "$target_sha"
```

If the push is rejected or `main` changes during the operation, stop and leave
the committed history for an explicit recovery. Never force-push, reset, amend,
or silently rebuild on a different SHA. For the initial release, leave the
version unchanged and set `target_sha` to the already verified clean `HEAD`.

## 5. Require exact push CI

Before creating or preparing a draft, prove that the ordinary `CI` workflow ran
from a `push` of `main` at `target_sha` and completed successfully. Recheck
that `git rev-parse HEAD` and `git ls-remote origin refs/heads/main` still equal
that SHA. Find a run only through the workflow endpoint, filter to
`event=push`, `head_branch=main`, `head_sha=target_sha`, and `name=CI`, and
choose the newest `(run_attempt, id)` pair. If it has not appeared yet, poll
for a bounded period; do not watch a run selected only by branch or by recency.

The essential API query is:

```bash
gh api --method GET "repos/$repo/actions/workflows/ci.yml/runs" \
  -f branch=main -f event=push -f head_sha="$target_sha" -f per_page=100
```

Once the exact numeric run ID is found, watch that ID and require its exit
status:

```bash
gh run watch "$ci_run_id" --exit-status
gh api "repos/$repo/actions/runs/$ci_run_id"
```

The final object must say `name=CI`, `event=push`, `head_branch=main`,
`head_sha=target_sha`, `status=completed`, and `conclusion=success`. A failed,
canceled, missing, or SHA-mismatched run stops before any draft is created.

## 6. Resolve the draft and tag state

Re-read both the exact tag refs and the exact Release object after CI. Query the
Release by tag through the API and distinguish a genuine 404 (absent) from
authentication or API errors. Never treat an error as absence. Require at most
one Release object for the exact tag.

Use this state matrix; every non-matching state is a conflict that stops:

| Tag | Release | Action |
| --- | --- | --- |
| absent | absent | Create a draft with `--target "$target_sha"`; do not require the tag to exist first. |
| absent | exact draft | Reuse only if its target resolves to `target_sha`; re-read after any API race. |
| exact lightweight/annotated | absent | Create the draft only if its peeled SHA is `target_sha`. |
| exact lightweight/annotated | exact draft | Reuse it; never recreate or move the tag. |
| conflict/ambiguous | any | Stop; do not delete, move, or overwrite the tag or Release. |
| exact tag | published | Verify the complete final state; never delete or edit it. |

An exact draft must have `tagName="$tag"`, `targetCommitish` resolving to the
full `target_sha`, `draft=true`, and `prerelease=false`. On creation use the
notes file and an explicit target; do not use generated notes or inline
escaped-newline arguments:

```bash
gh release create "$tag" \
  --draft \
  --title "$tag" \
  --target "$target_sha" \
  --notes-file "$body_file"
```

Do not pass `--verify-tag` in the absent-tag branch: the publisher explicitly
allows an absent tag and validates the post-creation tag state. After create,
re-fetch the tag refs and Release object and require the exact draft identity.
Capture its database ID for later retry checks.

## 7. Produce and verify the exact asset pair

The producer must run on the same clean arm64 macOS host, against the same
checked-out `target_sha` and synchronized contract:

```bash
bash scripts/build-macos-release.sh --upload "$tag"
```

The command builds an ad-hoc-signed Apple Silicon DMG, verifies the mounted
`CCResDoc.app`, stages `release-artifacts/CCResDoc_<version>_aarch64.dmg` and
its `.sha256`, checks the checksum, and uploads to the existing matching draft.
It will refuse a draft that already has managed assets. For a retry, use
`--clobber` only after the remote draft has the same complete pair (and no
other managed asset), the draft ID/target/tag still match, and the local exact
pair has been independently checked or intentionally removed:

```bash
bash scripts/build-macos-release.sh --upload "$tag" --clobber
```

If the draft has a partial pair, an unexpected managed name, an extra asset, or
a changed identity, stop. Do not use `--clobber` to repair a conflict. The
workflow requires the entire Release asset inventory to be exactly these two
names:

```text
CCResDoc_<version>_aarch64.dmg
CCResDoc_<version>_aarch64.dmg.sha256
```

Re-read the Release and verify the two assets are `uploaded`, have their
expected byte sizes and SHA-256 digests, and are the only assets. Download the
checksum and DMG to a fresh narrowly scoped temporary directory and run:

```bash
gh release view "$tag" --json url,databaseId,isDraft,isPrerelease,tagName,targetCommitish,assets
(cd "$verify_dir" && shasum -a 256 -c "$checksum_name")
```

The checksum must be one lowercase GNU-compatible line naming the exact DMG.
If a retry has a stale local pair, delete only the two exact paths from the
current contract after the remote pair is verified; the producer will then
rebuild them. Never delete a broad directory to make a retry pass.

## 8. Publish only the exact watched run

After the draft and pair pass all checks, a normal invocation dispatches the
checked-in publication workflow. Generate a fresh, unique request ID for every
dispatch; never reuse one from a previous attempt:

```bash
request_id="l-make-release-${tag#v}-$(node -e 'process.stdout.write(require("node:crypto").randomUUID())')"
```

Recheck `main` is still `target_sha`, then dispatch with the exact tag and SHA,
an explicit default-branch ref, and the required boolean value:

```bash
gh workflow run release.yml \
  --ref main \
  -f tag="$tag" \
  -f target_sha="$target_sha" \
  -f request_id="$request_id" \
  -f validation_only=false
```

The workflow's run name is the stable correlation key:

```text
Release <tag> @ <target_sha> [<request_id>]
```

Capture a returned URL if the CLI provides one. Otherwise poll the Release
workflow runs only until one run has exactly that display/run name, exact
`target_sha`, `event=workflow_dispatch`, and `head_branch=main`; extract its
numeric database ID. A run selected by tag alone, branch alone, or latest-run
ordering is not acceptable. Watch that exact ID and require exit status:

```bash
gh run watch "$release_run_id" --exit-status
gh run view "$release_run_id" --json status,conclusion,headSha,event,headBranch,displayTitle,url
```

If the watch fails, inspect the exact run and then inspect the Release state.
The expected recovery is to reuse the exact draft after resolving the reported
failure; do not dispatch a different SHA or delete a published Release. The
workflow itself revalidates `main`, CI, tag peel, draft identity, exact asset
IDs, checksum contents, and the pre/post publication state before making its
single publish mutation.

`--confirm` stops before this section. It reports a verified draft URL and
unpublished state; no publication workflow run is created.

## 9. Verify and report the live Release

After a successful exact workflow run, query the live Release by the exact tag
and require all of the following:

- `draft=false`, `prerelease=false`, and the Release is Latest;
- `tagName="$tag"` and `targetCommitish` is the full `target_sha`;
- the remote tag is present and peels to `target_sha` (report the peeled SHA);
- the complete asset inventory is exactly the DMG and its `.sha256`; and
- the downloaded checksum verifies the downloaded DMG with
  `shasum -a 256 -c`.

Report the live Release URL, tag and peeled SHA, target SHA, both exact asset
names, the checksum digest, and the producer's signing result. The expected
producer result is ad-hoc signing and the explicit
`gatekeeper=not-asserted-ad-hoc-signature` status. This is Apple Silicon-only
distribution. It is not Developer ID signed and is not notarized; explain that
Gatekeeper may require right-click **Open** on first launch or approval in
**System Settings → Privacy & Security**. Do not describe this workflow as
supporting Developer ID, notarization, or additional architectures.

If the exact published state already existed when a retry began, this same
verification is a successful no-op. If any published field, peeled SHA, asset
name, asset count, or checksum conflicts, stop and preserve it; never delete or
rewrite a published Release or tag.

## Cancel and safe recovery

`/l-make-release cancel` begins by repeating preflight, reading the current
contract, and resolving the exact current tag and Release. It is not a general
cleanup command.

1. If the Release is absent, report that there is nothing to cancel. Do not
   guess another tag.
2. If it is published, or if its tag/target/prerelease state is not an exact
   match, stop without deletion. A published Release is never cancelable.
3. If it is an exact draft, prove `draft=true`, `prerelease=false`, exact
   `tagName`, exact target SHA, and an unambiguous tag peel (absent or exact
   target as permitted by the state matrix). Record the Release database ID.
4. Delete only that verified draft by its recorded Release ID, then verify the
   Release is gone. Remove its tag only when the tag was verified to belong to
   that unpublished draft and is still not associated with a published
   Release; use narrowly targeted operations, never a wildcard:

   ```bash
   gh api --method DELETE "repos/$repo/releases/$release_id"
   # Only when the verified exact tag is present and safe to remove:
   gh api --method DELETE "repos/$repo/git/refs/tags/$tag"
   ```

   Re-query the Release between these operations and stop if it is not absent
   or if any published association appears.
5. If the release bump commit is still `HEAD` and there are no commits above
   it, prove this from the commit's exact three changed version paths and the
   current contract before reverting. Create a new revert commit with
   `git revert --no-edit <bump-sha>`, push it with ordinary `git push origin
   main`, and verify remote `main`. Never reset, amend, or force-push.
6. If any later commit exists above the bump, leave history and version files
   intact for the next release. Report that only the verified draft was
   canceled; do not revert someone else's later work.

If deletion or the guarded revert push fails, stop and report the exact state
for manual recovery. A local revert commit is not permission to force-push.
After cancel, verify there is no matching draft and no accidentally published
Release; if a tag cannot be proven safe to remove, leave it and report it
instead of deleting it.
