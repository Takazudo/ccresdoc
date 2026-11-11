import assert from "node:assert/strict";
import test from "node:test";
import { createSseParser } from "./sse-parser.mjs";

function collect(chunks) {
  const events = [];
  const parse = createSseParser((event) => events.push(event));
  for (const chunk of chunks) parse(chunk);
  return { events, partial: parse.end() };
}

test("split CRLF across chunks does not dispatch a premature empty frame", () => {
  const { events, partial } = collect(["event: page\r", "\ndata: {}\r\n\r\n"]);
  assert.deepEqual(events, [{ event: "page", data: "{}" }]);
  assert.equal(partial, false);
});

const frames = "event: page\r\ndata: {}\r\n\r\n: keep-alive\n\nevent: islands\rdata: A\rdata: B\r\rdata: x\n\n";
const expected = [
  { event: "page", data: "{}" },
  { event: "islands", data: "A\nB" },
  { event: "message", data: "x" },
];

test("every two-chunk split position yields the same complete frames", () => {
  for (let split = 0; split <= frames.length; split += 1) {
    const { events, partial } = collect([frames.slice(0, split), frames.slice(split)]);
    assert.deepEqual(events, expected, `split at ${split}`);
    assert.equal(partial, false, `split at ${split}`);
  }
});

test("one character per chunk yields the same complete frames", () => {
  assert.deepEqual(collect([...frames]).events, expected);
});

test("LF, CRLF and lone CR line endings are equivalent", () => {
  for (const eol of ["\n", "\r\n", "\r"]) {
    const text = ["event: css", "data: one", "data: two", "", ""].join(eol);
    assert.deepEqual(collect([text]).events, [{ event: "css", data: "one\ntwo" }], JSON.stringify(eol));
  }
});

test("a frame is dispatched only after its blank line arrives", () => {
  const events = [];
  const parse = createSseParser((event) => events.push(event));
  parse("event: page\ndata: {}\n");
  assert.deepEqual(events, []);
  parse("\n");
  assert.deepEqual(events, [{ event: "page", data: "{}" }]);
});

test("disconnect discards an unterminated frame and reports it", () => {
  const { events, partial } = collect(["event: page\ndata: {}\n\nevent: islands\ndata: late\n"]);
  assert.deepEqual(events, [{ event: "page", data: "{}" }]);
  assert.equal(partial, true);
  assert.equal(collect(["event: page\ndata: {}\n\n"]).partial, false);
  assert.equal(collect(["data: {}\n\nevent: pa"]).partial, true);
});

test("a named frame without a data line is not dispatched", () => {
  assert.deepEqual(collect(["event: page\n\n"]).events, []);
});

test("field values keep everything after the first colon and one optional space", () => {
  assert.deepEqual(collect(["data:  two spaces: kept\n\n"]).events, [{ event: "message", data: " two spaces: kept" }]);
  assert.deepEqual(collect(["data\n\n"]).events, [{ event: "message", data: "" }]);
});

test("empty chunks between a trailing CR and its LF keep the CRLF pair", () => {
  assert.deepEqual(collect(["data: a\r", "", "\ndata: b\n\n"]).events, [{ event: "message", data: "a\nb" }]);
});
