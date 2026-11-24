# ZUDO_DEPS_PINS

Provenance for first-party code adapted into CCResDoc. Keep this entry with the local implementation when the package tuple changes.

## zudo-doc Find in Page
- repo: zudolab/zudo-doc
- what: desktop Find implementation adapted into the app after zudo-doc retired its Tauri Find export
- files: app/src/browser-chrome/find-in-page/
- source: packages/zudo-doc/dist/find-in-page/ (published package bytes)
- track: releases
- pinned: 76e44dbc058f4876ca9ffd09b5151deb95d3bfbe (zudo-doc-v10.0.2)
- updated: 2026-11-25
- sync: manually compare the published package and local behavior before adopting changes
- notes: The source was the published 10.0.2 Find implementation with CCResDoc's former controlled patch applied. Local changes add command helpers, host-owned shortcut opt-out, WebKit search-input attributes, and chrome/control exclusion. zudo-doc 11 and later removed this component; do not replace it with a newer package file. Keep package Search separate.
