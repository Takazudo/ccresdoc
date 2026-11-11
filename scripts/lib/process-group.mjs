import { spawnSync } from "node:child_process";
import { setTimeout as delay } from "node:timers/promises";

/**
 * Parse `ps -eo pid=,pgid=,stat=,args=` output into the live members of one
 * process group. Zombie rows (stat Z) are not live: an orphan that exited is
 * reported as `<defunct>` until its reaper collects it, which some container
 * init processes never do.
 */
export function parseGroupMembers(psOutput, pgid) {
  return psOutput
    .split("\n")
    .map((line) => /^\s*(\d+)\s+(\d+)\s+(\S+)\s*(.*)$/.exec(line))
    .filter(Boolean)
    .map(([, pid, group, stat, args]) => ({ pid: Number(pid), pgid: Number(group), stat, args }))
    .filter((row) => row.pgid === pgid && !row.stat.startsWith("Z"));
}

export function liveGroupMembers(pgid) {
  const result = spawnSync("ps", ["-eo", "pid=,pgid=,stat=,args="], { encoding: "utf8" });
  if (result.status !== 0) throw new Error(`ps failed: ${result.stderr}`);
  return parseGroupMembers(result.stdout, pgid);
}

/** Wait until no live process remains in the group; returns the survivors. */
export async function waitForEmptyGroup(pgid, { timeoutMs = 5_000, read = liveGroupMembers } = {}) {
  const deadline = Date.now() + timeoutMs;
  let members = read(pgid);
  while (members.length > 0 && Date.now() < deadline) {
    await delay(100);
    members = read(pgid);
  }
  return members;
}
