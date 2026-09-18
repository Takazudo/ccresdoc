import assert from "node:assert/strict";
import manual from "../configs/manual.mjs";
import routesOff from "../configs/routes-off.mjs";
import selected from "../configs/selected.mjs";
import wholesale from "../configs/wholesale.mjs";

const names = (config) => (config.plugins ?? []).map((plugin) => plugin.name);

// zudo-doc 5.25.0's preset adds `img-src-check` and, because `designTokenPanel`
// is off in the probe settings, the `zdtp-loader` shield. That shield is what
// keeps the package-owned configs building without `@takazudo/zdtp` installed;
// the `plugins: []` configs below drop it, which is why the app carries the
// equivalent stub in its own zudo-doc patch.
assert.deepEqual(names(wholesale), [
  "@takazudo/zudo-doc/plugins/routes",
  "@takazudo/zudo-doc/plugins/search-index",
  "@takazudo/zudo-doc/plugins/theme-packs",
  "@takazudo/zudo-doc/plugins/img-src-check",
  "@takazudo/zudo-doc/plugins/zdtp-loader",
]);
assert.deepEqual(names(routesOff), [
  "@takazudo/zudo-doc/plugins/search-index",
  "@takazudo/zudo-doc/plugins/theme-packs",
  "@takazudo/zudo-doc/plugins/img-src-check",
  "@takazudo/zudo-doc/plugins/zdtp-loader",
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
