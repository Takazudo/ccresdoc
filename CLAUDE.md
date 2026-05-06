# CCResDoc

This repo is a restructure of the existing ccresdoc Tauri app (originally at $HOME/.claude/doc/) using zfb (Rust SSG orchestrator at $HOME/repos/myoss/zfb). The hybrid architecture is documented in the epic issue.

## When working on this repo

- For zfb-related work, **invoke `/refer-another-project zfb` first** so you pick up zfb's repo structure, crate layout, and CLAUDE.md context. zfb is a real-world test of this restructure — bugs found in zfb may be fixed upstream by PR + merge to zfb's main, per project authorisation.
- For Tauri work, consult `/tauri-wisdom` (esp. the `recipes/doc-viewer-app.mdx` recipe).

### zfb upstream edits — ALWAYS use a git worktree

The zfb repo at `$HOME/repos/myoss/zfb` is shared. Other Claude Code sessions may be developing in it concurrently — running `git checkout`, merging branches, leaving uncommitted work in the working tree. Editing the repo root directly causes branch-switch races and surprise file modifications across sessions.

**Rule:** never run `git checkout`, `git stash`, or commit directly in `$HOME/repos/myoss/zfb`. Create a dedicated worktree first.

**Pattern:**

```bash
# 1. Pick a worktree path under ccresdoc (gitignored)
WT=$HOME/repos/myoss/ccresdoc/__inbox/zfb-wt-<topic>

# 2. Create the worktree from the latest origin/main
cd "$HOME/repos/myoss/zfb"
git fetch origin --prune
git worktree add -b fix/<topic> "$WT" origin/main

# 3. Do all work inside $WT — edit, test, commit, push, gh pr create, merge.
#    The repo root is never touched.

# 4. After the upstream PR merges, rebuild zfb FROM THE WORKTREE so the
#    binary picks up exactly the merged tree (avoids race with whatever the
#    other agent did to the repo root):
cargo install --path "$WT/crates/zfb"

# 5. When done, clean up:
cd "$HOME/repos/myoss/zfb"
git worktree remove "$WT"
git branch -D fix/<topic>   # only after the PR is merged on origin
```

**Always rebuild ccresdoc against the latest zfb main** after upstream merges land — the consumer code here assumes whatever is on `origin/main` of zfb. After any zfb PR merges, run `cargo install --path <zfb-wt>/crates/zfb` (or pull main into a fresh worktree) and re-run `bash scripts/run-b4push.sh` before pushing ccresdoc changes.

## Per-directory context files

Detailed architecture notes are in per-directory CLAUDE.md files — read these before touching a subdirectory:

- `crates/CLAUDE.md` — Rust workspace layout; what each crate owns
- `src-tauri/CLAUDE.md` — Tauri wrapper architecture (embedded axum server, window lifecycle)
- `app/CLAUDE.md` — zfb frontend project; embedded package credits; known zfb workarounds
