import assert from "node:assert/strict";
import manual from "../configs/manual.mjs";
import routesOff from "../configs/routes-off.mjs";
import selected from "../configs/selected.mjs";
import wholesale from "../configs/wholesale.mjs";

const names = (config) => (config.plugins ?? []).map((plugin) => plugin.name);

// zudo-doc 10.0.2's preset adds `img-src-check` plus three disabled-feature
// loader shields (`zdtp-loader`, `katex-loader`, `diff-loader`) for features
// the probe settings leave off. The shields keep package-owned configs
// building without those optional peers; the `plugins: []` configs below drop
// them, which is why the app carries a zdtp-loader stub in its own zudo-doc
// patch. katex is a declared runtime dependency and the diff edge is a
// server-side dynamic import with a null fallback, so neither needs a stub.
assert.deepEqual(names(wholesale), [
  "@takazudo/zudo-doc/plugins/routes",
  "@takazudo/zudo-doc/plugins/search-index",
  "@takazudo/zudo-doc/plugins/theme-packs",
  "@takazudo/zudo-doc/plugins/img-src-check",
  "@takazudo/zudo-doc/plugins/zdtp-loader",
  "@takazudo/zudo-doc/plugins/katex-loader",
  "@takazudo/zudo-doc/plugins/diff-loader",
]);
assert.deepEqual(names(routesOff), [
  "@takazudo/zudo-doc/plugins/search-index",
  "@takazudo/zudo-doc/plugins/theme-packs",
  "@takazudo/zudo-doc/plugins/img-src-check",
  "@takazudo/zudo-doc/plugins/zdtp-loader",
  "@takazudo/zudo-doc/plugins/katex-loader",
  "@takazudo/zudo-doc/plugins/diff-loader",
]);
assert.deepEqual(names(selected), []);
assert.deepEqual(names(manual), []);

for (const config of [wholesale, routesOff, selected, manual]) {
  assert.equal(config.port, 4892);
  assert.equal(config.base, "/");
  assert.equal(config.trailingSlash, true);
  assert.equal(config.collections[0].name, "docs");
  assert.equal(config.collections[0].path, "src/content/docs");
  assert.equal(config.markdown.features.headingIds.strategy, "hierarchical");
}

assert.deepEqual(selected.collections, routesOff.collections);
assert.deepEqual(selected.markdown, routesOff.markdown);
assert.deepEqual(selected.resolveMarkdownLinks, routesOff.resolveMarkdownLinks);

console.log("config matrix assertions passed");
