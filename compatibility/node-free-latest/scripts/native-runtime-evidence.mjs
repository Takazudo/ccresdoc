import assert from "node:assert/strict";

// zfb dev publishes page and islands frames for one content change in either
// order; both prove the reload stream observed it.
export const CONTENT_HMR_MARKER = "HMR reached the native route at 4892.";

export const CONTENT_RELOAD_EVENTS = new Set(["page", "islands"]);

/**
 * Assert the node-free ownership of one sampled zfb process group and reduce it
 * to its semantic outcome. PIDs, compiler-worker rows and their count vary
 * between runs, so none of them reach the evidence.
 */
export function processGroupOutcome(members, leaderPid) {
  assert.ok(members.some(({ pid }) => pid === leaderPid), "the native zfb leader must own its process group");
  assert.equal(members.some(({ args }) => /plugin-host\.mjs/.test(args)), false, "no Node plugin host may run");
  assert.equal(members.some(({ args }) => /(?:^|\/)node(?:\s|$)/.test(args)), false, "no Node process may run");
  return { nativeZfbLeader: true, nodePluginHost: false, nodeProcess: false };
}

/** The probe's successful evidence document; every field is run-invariant. */
export function nativeRuntimeEvidence({ reloadEvent, processGroup }) {
  assert.ok(CONTENT_RELOAD_EVENTS.has(reloadEvent), `unexpected content reload event: ${reloadEvent}`);
  return {
    status: "passed",
    homeRoute: "/",
    docsRoute: "/docs/probe/",
    islandMarker: "ProbeCounter",
    contentHmrMarker: CONTENT_HMR_MARKER,
    contentReloadObserved: true,
    nodeSentinelInvocations: 0,
    fixedPortSerialized: true,
    portReleasedAfterTeardown: true,
    processGroup,
  };
}
