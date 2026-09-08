import assert from "node:assert/strict";
import test from "node:test";
import { createProbeOutputNormalizer } from "./normalize-probe-output.mjs";

const suffix = "/node_modules/@takazudo/zudo-doc/dist/doc-history/index.js:97:37:";

for (const root of ["/tmp/checkout/compatibility", "/private/tmp/checkout/compatibility", "/var/tmp/checkout/compatibility", "/private/var/tmp/checkout/compatibility"]) {
  test(`Darwin absolute and relative aliases for ${root}`, () => {
    const normalize = createProbeOutputNormalizer({ probeRoot: root, tempRoot: "/var/folders/test/T", platform: "darwin" });
    const shortRoot = root.replace(/^\/private/, "");
    for (const spelling of [shortRoot, `/private${shortRoot}`]) {
      assert.equal(normalize(`${spelling}${suffix}`), `<probe-root>${suffix}`);
      assert.equal(normalize(`  ../../../../../${spelling.slice(1)}${suffix}`), `  <probe-root>${suffix}`);
    }
  });
}

test("does not manufacture aliases for arbitrary private directories or neighboring paths", () => {
  const normalize = createProbeOutputNormalizer({ probeRoot: "/private/project", tempRoot: "/private/cache", platform: "darwin" });
  assert.equal(normalize(`/project${suffix}`), `/project${suffix}`);
  assert.equal(normalize(`/private/project-other${suffix}`), `/private/project-other${suffix}`);
  assert.equal(normalize(`/private/project${suffix}`), `<probe-root>${suffix}`);
  assert.equal(normalize("/cache/zfb-plugin-host-123/main.mjs"), "/cache/zfb-plugin-host-123/main.mjs");
});

test("Linux keeps distinct /private paths and escapes special path characters", () => {
  const root = "/private/tmp/checkout[1]/compatibility";
  const normalize = createProbeOutputNormalizer({ probeRoot: root, tempRoot: "/tmp", platform: "linux" });
  assert.equal(normalize(`${root}${suffix}`), `<probe-root>${suffix}`);
  assert.equal(normalize(`../../../${root.slice(1)}${suffix}`), `<probe-root>${suffix}`);
  assert.equal(normalize(`/tmp/checkout[1]/compatibility${suffix}`), `/tmp/checkout[1]/compatibility${suffix}`);
});

test("normalizes macOS temporary workspace and plugin-host spellings", () => {
  for (const tempRoot of ["/var/folders/test/T", "/private/var/folders/test/T/"]) {
    const normalize = createProbeOutputNormalizer({ probeRoot: "/repo/probe", tempRoot, platform: "darwin" });
    for (const prefix of ["/var/folders/test/T", "/private/var/folders/test/T"]) {
      assert.equal(normalize(`${prefix}/ccresdoc-config-wholesale-123/workspace/page.ts`), "<isolated-workspace>/page.ts");
      assert.equal(normalize(`${prefix}/zfb-plugin-host-123/main.mjs in 12.34s`), "<zfb-plugin-host>/main.mjs in <duration>");
    }
  }
});

test("Windows probe paths retain backslash suffix normalization", () => {
  const root = String.raw`C:\work\probe`;
  const normalize = createProbeOutputNormalizer({ probeRoot: root, tempRoot: String.raw`C:\Temp`, platform: "win32" });
  assert.equal(normalize(`${root}\\node_modules\\package.js:10:2`), String.raw`<probe-root>\node_modules\package.js:10:2`);
});

test("does not partially redact an unrelated /private prefix on Linux", () => {
  const normalize = createProbeOutputNormalizer({ probeRoot: "/tmp/probe", tempRoot: "/tmp", platform: "linux" });
  assert.equal(normalize(`/private/tmp/probe${suffix}`), `/private/tmp/probe${suffix}`);
});
