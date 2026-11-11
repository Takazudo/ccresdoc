import assert from "node:assert/strict";
import test from "node:test";
import { parseGroupMembers, waitForEmptyGroup } from "./process-group.mjs";

const ps = [
  "  100   100 Ssl  /work/node_modules/@takazudo/zfb-linux-x64-gnu/zfb dev --port 4892",
  "  101   100 Z    [zfb] <defunct>",
  "  102   100 S    /work/node_modules/@takazudo/zfb-linux-x64-gnu/zfb compile-worker",
  "  200   200 S    unrelated",
  "  300   100Z bad row",
].join("\n");

test("group members exclude zombies, other groups and malformed rows", () => {
  assert.deepEqual(parseGroupMembers(ps, 100).map(({ pid }) => pid), [100, 102]);
  assert.deepEqual(parseGroupMembers(ps, 999), []);
});

test("waiting returns once the group is empty and reports survivors on timeout", async () => {
  let reads = 0;
  const draining = () => (++reads < 3 ? [{ pid: 102 }] : []);
  assert.deepEqual(await waitForEmptyGroup(100, { read: draining, timeoutMs: 2_000 }), []);
  assert.deepEqual(await waitForEmptyGroup(100, { read: () => [{ pid: 102 }], timeoutMs: 150 }), [{ pid: 102 }]);
});
