---
name: ccresdoc-build
description: "Compatibility alias for the project-local /l-build workflow. Prefer /l-build for building, verifying, installing, launching, and confirming CCResDoc.app."
user-invocable: true
allowed-tools:
  - Bash(pnpm rebuild:local-app*)
  - Bash(SKIP_APP_BUILD=1 pnpm rebuild:local-app*)
---

# Compatibility alias

Use `/l-build`. Its implementation is `.claude/skills/l-build/SKILL.md`, backed
by the single project command:

```bash
pnpm rebuild:local-app
```
