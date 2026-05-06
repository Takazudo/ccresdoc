---
name: zfb-upstream-dev
description: "Develop, build, or fix the upstream zfb crate without disturbing $HOME/repos/myoss/zfb's working tree. Use when: (1) Editing zfb source as part of a ccresdoc task, (2) Filing a zfb PR / merging zfb fixes, (3) Bumping pinned versions (esbuild, tailwindcss, framework runtimes), (4) Anything that runs `git checkout` / `git stash` / `git commit` / `cargo install` against zfb. Mandatory because the zfb checkout at $HOME/repos/myoss/zfb is shared with other Claude Code sessions; touching its working tree races their state."
---

# zfb Upstream Development — worktree mandatory

The zfb repo at `$HOME/repos/myoss/zfb` is **shared** between concurrent Claude Code sessions. Any state-mutating git operation in its repo root (checkout, branch, merge, stash, commit, even an inadvertent uncommitted edit) can land on whichever branch a peer session just switched to, drag uncommitted edits across branches, or fight a peer's `git checkout` mid-build. Lost work is the typical outcome.

## The rule

**Never run state-mutating git ops at `$HOME/repos/myoss/zfb` directly.** That includes — at minimum:

- `git checkout` / `git switch`
- `git branch -d` / `-D` / branch creation
- `git commit` / `git add`
- `git merge` / `git rebase` / `git cherry-pick`
- `git stash` (push or pop)
- `git pull` / `git push` (push only — fetch is read-only and safe)
- `git reset` (any flavour) / `git restore` (any flavour)
- File edits in `$HOME/repos/myoss/zfb/` (with `Write`, `Edit`, `sed -i`, `cargo fmt`, codegen, anything that writes to the working tree)
- `cargo install --path crates/zfb` from the repo root (the build script writes back into `crates/zfb/binaries/`; that mutates the shared tree)

For any of these, **create a fresh worktree first**.

Read-only ops at the repo root are fine: `git fetch`, `git log`, `git diff`, `git show`, `git worktree list`, plus any `Read` / `grep` / `find` against the source tree. Use these freely for inspection.

## Picking a worktree path

Any **fresh, unused** path works. Two reasonable conventions:

- **Sibling under `$HOME/repos/myoss/`** — visible alongside other myoss repos, easy to `cd` to:

  ```
  $HOME/repos/myoss/zfb-wt-<topic>
  ```

- **Under `$HOME/.claude/worktrees/zfb/`** — keeps `myoss/` clean, matches the layout `/x-wt-teams` uses for ephemeral worktrees:

  ```
  $HOME/.claude/worktrees/zfb/<topic>
  ```

Pick whichever is convenient. The path is **not** in `__inbox/` (that's ccresdoc's local scratch space, not for cross-repo worktrees) and **not** under `$HOME/repos/myoss/zfb/` itself (that would be inside the repo).

`<topic>` is a short hyphen-cased slug: `runtime-check-fix`, `bump-esbuild-0-25-13`, etc.

## Recipes

### Recipe A — Read-only inspection

Goal: read zfb source, search for a symbol, view recent history. **No worktree needed.**

```bash
# Always safe at the repo root:
cd $HOME/repos/myoss/zfb
git fetch origin --prune                 # remote refs only — no working-tree write
git log --oneline -20 main
git log --oneline 8e8aed2..origin/main   # what landed on main since a known commit
git show <sha> --stat
grep -rn "embedded_node_modules" crates/zfb/src/
```

`Read`, `grep`, `find` against the working tree are all fine. The shared state hazard is only for ops that **mutate** the tree (checkout, edits, commits, pulls, builds-with-side-effects).

### Recipe B — Edit-and-build flow

Goal: edit zfb, run tests, commit, push, open a PR, merge, then rebuild the local binary so ccresdoc picks up the merged tree.

```bash
# 1. Pick a worktree path
WT=$HOME/repos/myoss/zfb-wt-<topic>

# 2. Fetch latest and create the worktree off origin/main
cd $HOME/repos/myoss/zfb
git fetch origin --prune
git worktree add -b fix/<topic> "$WT" origin/main

# 3. Symlink node_modules from the main checkout
#    zfb's build.rs reads `node_modules/.pnpm/<pkg>@<ver>/...` to embed
#    framework packages into the binary. The worktree shares git history
#    but not node_modules, so symlink it from the main checkout.
ln -s $HOME/repos/myoss/zfb/node_modules "$WT/node_modules"

# 4. Optional: pre-stage the binaries dir if the worktree's
#    `crates/zfb/binaries/tailwindcss-v4` is missing and you don't want
#    `build.rs` to re-download it. Skip on a fast network.
if [ ! -f "$WT/crates/zfb/binaries/tailwindcss-v4" ]; then
  cp $HOME/repos/myoss/zfb/crates/zfb/binaries/tailwindcss-v4 \
     "$WT/crates/zfb/binaries/tailwindcss-v4"
  chmod +x "$WT/crates/zfb/binaries/tailwindcss-v4"
fi

# 5. Work inside $WT — edit, test, commit, push:
cd "$WT"
# ... edits ...
cargo test -p zfb --lib
cargo test --workspace                         # full pre-PR sanity check
git add <files>
git commit -m "fix: <message>"
git push -u origin fix/<topic>

# 6. Open the PR. Always pass `--head` explicitly so it works even when a
#    peer agent has switched the zfb repo root to a different branch in
#    between your push and your `gh pr create`.
gh pr create --repo Takazudo/zudo-front-builder \
  --head fix/<topic> --base main \
  --title "fix(...): <summary>" \
  --body "..."

# 7. After CI passes, merge:
gh pr merge <PR_NUMBER> --repo Takazudo/zudo-front-builder \
  --merge --delete-branch

# 8. Rebuild the local zfb binary FROM THE WORKTREE so the cargo-installed
#    binary at $HOME/.cargo/bin/zfb matches exactly what just merged. Doing
#    this from the repo root would race a peer agent that may have switched
#    its branch while you were merging.
cargo install --path "$WT/crates/zfb"

# 9. Clean up the worktree (always from the repo root, never from inside $WT):
cd $HOME/repos/myoss/zfb
git worktree remove --force "$WT"

# 10. Verify the consumer still passes:
cd $HOME/repos/myoss/ccresdoc
unset ZFB_HOME ZFB_ESBUILD_BIN ZFB_TAILWIND_BIN
bash scripts/run-b4push.sh
```

Local `fix/<topic>` branch may linger after merge if `git branch -d` from the repo root reports "not fully merged" (the merge happened on the remote; your local main is behind). Pull main first, then delete:

```bash
cd $HOME/repos/myoss/zfb
git fetch origin
git checkout main 2>/dev/null && git pull --ff-only origin main
git branch -d fix/<topic>
```

The `git checkout main` step is the **one exception** to the no-checkout rule — and it's still risky if a peer is mid-edit. Defer this cleanup; a stale local branch costs nothing. Only do it during a quiet window.

### Recipe C — Pin bumps (esbuild / tailwindcss / framework runtimes)

zfb's `crates/zfb/build.rs` pins constants that must stay in lockstep with other source-of-truth files:

- `ESBUILD_VERSION` ↔ `EXPECTED_ESBUILD_VERSION` in `crates/zfb-islands/src/esbuild.rs`
- `TAILWIND_VERSION` ↔ `TAILWIND_VERSION` in `scripts/fetch-tailwind.mjs`
- `PREACT_VERSION` / `PREACT_RTS_VERSION` / `HONO_VERSION` ↔ `pnpm-lock.yaml` (via `pnpm install`)

Pin bumps mutate `crates/zfb/binaries/*` SHA-256 constants and the version constants in lockstep with each other. The build script also re-downloads binaries during `cargo install`. Both mutate the working tree, so worktree is mandatory.

Workflow on top of Recipe B:

```bash
WT=$HOME/repos/myoss/zfb-wt-bump-<pkg>-<version>

# Setup as in Recipe B steps 1–3.

# Update the version constants in $WT/crates/zfb/build.rs AND the matching
# source-of-truth file (esbuild.rs / fetch-tailwind.mjs / pnpm-lock.yaml).
# Then update the SHA-256 constant in build.rs to match the new download.

# Confirm the SHA-256 by letting build.rs download once and reading the
# value it computes (mismatched SHAs are a hard build error so you can't
# accidentally ship the wrong hash):
cd "$WT" && cargo build -p zfb 2>&1 | grep "SHA-256"

# For pnpm-locked framework packages (preact / preact-render-to-string / hono),
# also bump the corresponding entry in $WT/pnpm-lock.yaml via:
cd "$WT" && pnpm install --lockfile-only

# Then continue with Recipe B steps 5–10. The cargo install step will
# re-stage the new binaries inside $WT/crates/zfb/binaries/ — confined to
# the worktree, the shared root tree is untouched.
```

After merge, downstream consumers (ccresdoc, others) just need a fresh `cargo install --path <wt>/crates/zfb` against the merged tree to pick up the new pin.

## Common pitfalls

- **`cd ../zfb && cargo install --path crates/zfb` from inside ccresdoc** — runs the build script in the shared tree, downloads binaries to it, and SHA-mismatches a peer's pin if their branch had different versions. Always install from a worktree.
- **`git stash` "to set things aside"** — both `push` and `pop` mutate the working tree and stash list. If you stashed at the root and a peer ran `git checkout`, your stash applies on the wrong branch. Use a worktree instead, or just commit to a fix branch.
- **`gh pr create` without `--head`** — `gh` infers the head branch from the current directory's HEAD. When the zfb root has been bounced to another branch by a peer, the inferred head is wrong. Always pass `--head fix/<topic>` explicitly.
- **Editing `crates/zfb/binaries/`** — the build script overwrites these on every build. Edits don't survive and a peer's `cargo install` will overwrite a hand-staged binary with the SHA-pinned download. Bump pins in `build.rs` constants instead.
- **Forgetting `node_modules` symlink** — without it, the worktree's `cargo build` panics in `build.rs` looking for `node_modules/.pnpm/preact@<ver>`. Recipe B step 3.

## When NOT to use a worktree

- Pure `Read` of zfb source files
- `grep` / `find` over the source tree
- `git fetch` (does not write the working tree)
- `git log` / `git show` / `git diff`
- Running `gh pr view` / `gh issue view` against the zfb repo

These are read-only and free of races. Do them at the repo root.
