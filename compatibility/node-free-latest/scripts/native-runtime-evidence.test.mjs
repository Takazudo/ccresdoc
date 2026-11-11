import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { serializeEvidence } from "./evidence-diff.mjs";
import { nativeRuntimeEvidence, processGroupOutcome } from "./native-runtime-evidence.mjs";

const committed = readFileSync(new URL("../evidence/native-runtime.json", import.meta.url), "utf8");
const zfb = "/work/node_modules/@takazudo/zfb-darwin-arm64/zfb";

function serialize(reloadEvent, members, leaderPid) {
  const processGroup = processGroupOutcome(members, leaderPid);
  return serializeEvidence(nativeRuntimeEvidence({ reloadEvent, processGroup }));
}

const groups = [
  { name: "leader only", leaderPid: 4100, members: [{ pid: 4100, args: `${zfb} dev --port 4892` }] },
  {
    name: "leader plus compiler workers",
    leaderPid: 51234,
    members: [
      { pid: 51234, args: `${zfb} dev --port 4892` },
      { pid: 51240, args: `${zfb} compile-worker` },
      { pid: 51241, args: `${zfb} compile-worker` },
    ],
  },
  {
    name: "worker listed before the leader",
    leaderPid: 7,
    members: [
      { pid: 9, args: `${zfb} compile-worker` },
      { pid: 7, args: `${zfb} dev --port 4892` },
    ],
  },
];

test("permissible reload ordering and process observations serialize to the committed evidence", () => {
  for (const reloadEvent of ["page", "islands"]) {
    for (const { name, leaderPid, members } of groups) {
      assert.equal(serialize(reloadEvent, members, leaderPid), committed, `${reloadEvent} / ${name}`);
    }
  }
});

test("a group the native leader does not own fails instead of drifting", () => {
  assert.throws(
    () => serialize("page", [{ pid: 4101, args: `${zfb} compile-worker` }], 4100),
    /the native zfb leader must own its process group/,
  );
});

test("a Node plugin host in the group fails", () => {
  const members = [
    { pid: 4100, args: `${zfb} dev --port 4892` },
    { pid: 4102, args: "/work/node_modules/@takazudo/zfb/plugin-host.mjs" },
  ];
  assert.throws(() => serialize("page", members, 4100), /no Node plugin host may run/);
});

test("a Node process in the group fails", () => {
  for (const args of ["node server.mjs", "/usr/local/bin/node", "/opt/homebrew/bin/node --version"]) {
    const members = [{ pid: 4100, args: `${zfb} dev --port 4892` }, { pid: 4103, args }];
    assert.throws(() => serialize("page", members, 4100), /no Node process may run/, args);
  }
});

test("a missing or foreign reload event fails", () => {
  const { members } = groups[0];
  for (const reloadEvent of [undefined, null, "", "reload", "connected"]) {
    assert.throws(() => serialize(reloadEvent, members, 4100), /unexpected content reload event/, String(reloadEvent));
  }
});
