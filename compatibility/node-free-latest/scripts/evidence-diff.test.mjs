import assert from "node:assert/strict";
import test from "node:test";
import { diffJson, formatEvidenceDrift } from "./evidence-diff.mjs";

const facts = {
  packages: [
    { name: "@takazudo/zfb", version: "6.0.1", integrity: "sha512-AAA", sha256: "aa11" },
    { name: "@takazudo/zudo-doc", version: "10.0.2", integrity: "sha512-BBB", sha256: "bb22" },
  ],
  contentReloadEvent: "page",
};

function changed(mutate) {
  const copy = structuredClone(facts);
  mutate(copy);
  return copy;
}

test("identical evidence has no differences", () => {
  assert.deepEqual(diffJson(facts, structuredClone(facts)), { differences: [], total: 0 });
});

test("a changed integrity hash is reported with its path and both values", () => {
  const next = changed((value) => { value.packages[1].integrity = "sha512-CCC"; });
  assert.deepEqual(diffJson(facts, next).differences, [
    { path: "$.packages[1].integrity", old: "sha512-BBB", new: "sha512-CCC" },
  ]);
  assert.match(formatEvidenceDrift("package-facts.json", JSON.stringify(facts), next), /\$\.packages\[1\]\.integrity: "sha512-BBB" -> "sha512-CCC"/);
});

test("a changed file hash or version is never treated as equal", () => {
  for (const mutate of [
    (value) => { value.packages[0].sha256 = "aa12"; },
    (value) => { value.packages[0].version = "6.0.2"; },
    (value) => { value.contentReloadEvent = "islands"; },
  ]) {
    assert.equal(diffJson(facts, changed(mutate)).total, 1);
  }
});

test("added and removed entries are reported as missing", () => {
  const next = changed((value) => { value.packages.pop(); value.extra = true; });
  assert.deepEqual(diffJson(facts, next).differences.map((entry) => entry.path), [
    "$.extra",
    "$.packages[1]",
  ]);
  const report = formatEvidenceDrift("x.json", JSON.stringify(facts), next);
  assert.match(report, /\$\.extra: <missing> -> true/);
  assert.match(report, /\$\.packages\[1\]: \{"name":"@takazudo\/zudo-doc".* -> <missing>/);
});

test("output is bounded in entry count and value length", () => {
  const before = { list: Array.from({ length: 50 }, (_, index) => index), long: "x".repeat(10) };
  const after = { list: Array.from({ length: 50 }, (_, index) => index + 1), long: "y".repeat(1000) };
  const report = formatEvidenceDrift("big.json", JSON.stringify(before), after, { limit: 5, maxValueLength: 40 });
  assert.equal(report.split("\n").length, 1 + 5 + 1);
  assert.match(report, /51 changed values/);
  assert.match(report, /… 46 more not shown/);
  const longReport = formatEvidenceDrift("big.json", JSON.stringify({ long: "x" }), { long: "y".repeat(1000) }, { maxValueLength: 40 });
  assert.match(longReport, /… \(1002 chars\)/);
});

test("serialization-only differences are named as such", () => {
  assert.match(formatEvidenceDrift("a.json", JSON.stringify(facts), facts), /same JSON value, different serialization/);
  assert.match(formatEvidenceDrift("a.json", "{not json", facts), /not valid JSON/);
});

test("values that do not serialize as JSON are compared as they serialize", () => {
  assert.match(formatEvidenceDrift("x.json", '{"a":1}', { a: 2, b: undefined }), /^x\.json: 1 changed value\n  \$\.a: 1 -> 2$/);
  assert.match(formatEvidenceDrift("x.json", '{"a":1}', { a: Number.NaN }), /\$\.a: 1 -> null/);
});
