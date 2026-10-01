import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { afterEach, test } from "node:test";
import { EventEmitter } from "node:events";
import {
  compareDocumentCheckpoint,
  createSseParser,
  formatDiagnostics,
  guardDocumentReloads,
  installAttributeRecorder,
  isContextDestroyedError,
  plantDocumentId,
  probePlantedState,
  readPlantedState,
  recordDiagnostics,
  waitForDevSettled,
} from "./browser-harness.mjs";

const harnessUrl = pathToFileURL(resolve(dirname(fileURLToPath(import.meta.url)), "browser-harness.mjs")).href;
const servers = [];

afterEach(async () => {
  while (servers.length) await servers.pop().stop();
});

// Fake zfb dev server: /__zfb/ready, /docs/ with x-zfb-dev-generation, and
// an SSE /__zfb/reload whose clients the test can emit to or drop.
async function startFakeZfb({ ready = true, readyStatus = 200, generation = "1" } = {}) {
  const state = { ready, readyStatus, generation, sseConnections: 0, sseClosed: 0, refuseSse: false };
  const sseClients = new Set();
  const server = createServer((req, res) => {
    const path = new URL(req.url, "http://x").pathname;
    if (path === "/__zfb/ready") {
      if (state.readyStatus === 404) {
        res.writeHead(404).end("not found");
        return;
      }
      res.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
      res.end(JSON.stringify({ generation: Number(state.generation ?? 0), ready: state.ready, documents: state.ready ? "published" : "pending" }));
      return;
    }
    if (path === "/__zfb/reload") {
      if (state.refuseSse) {
        res.writeHead(503).end();
        return;
      }
      state.sseConnections += 1;
      res.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-cache" });
      res.write(": connected\n\n");
      sseClients.add(res);
      req.on("close", () => {
        state.sseClosed += 1;
        sseClients.delete(res);
      });
      return;
    }
    if (path === "/docs/") {
      const headers = { "content-type": "text/html; charset=utf-8" };
      if (state.generation !== null) headers["x-zfb-dev-generation"] = state.generation;
      res.writeHead(200, headers).end("<!doctype html><title>CCResDoc</title>");
      return;
    }
    res.writeHead(404).end();
  });
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  const baseUrl = `http://127.0.0.1:${server.address().port}`;
  const fake = {
    baseUrl,
    state,
    emit(event, data = "") {
      for (const client of sseClients) client.write(`event: ${event}\ndata: ${data}\n\n`);
    },
    dropSse() {
      for (const client of sseClients) client.destroy();
    },
    openSseClients: () => sseClients.size,
    async stop() {
      for (const client of sseClients) client.destroy();
      server.closeAllConnections();
      await new Promise((done) => server.close(done));
    },
  };
  servers.push(fake);
  return fake;
}

const delay = (ms) => new Promise((done) => setTimeout(done, ms));

async function waitFor(predicate, timeoutMs = 2000) {
  const deadline = Date.now() + timeoutMs;
  while (!predicate()) {
    if (Date.now() > deadline) throw new Error("waitFor timed out");
    await delay(10);
  }
}

const fast = { quietMs: 300, pollMs: 25, fetchTimeoutMs: 1000 };

test("settles on a quiet ready server and closes the SSE stream", async () => {
  const zfb = await startFakeZfb();
  const result = await waitForDevSettled(zfb.baseUrl, { ...fast, timeoutMs: 5000 });
  assert.equal(result.generation, "1");
  assert.equal(result.readyPayload.ready, true);
  assert.equal(zfb.state.sseConnections, 1);
  await waitFor(() => zfb.openSseClients() === 0);
});

test("a generation change resets the quiet window", async () => {
  const zfb = await startFakeZfb();
  setTimeout(() => {
    zfb.state.generation = "2";
  }, 200);
  const result = await waitForDevSettled(zfb.baseUrl, { ...fast, timeoutMs: 5000 });
  assert.equal(result.generation, "2");
  assert(result.elapsedMs >= 200 + fast.quietMs, `settled too early: ${result.elapsedMs}ms`);
  assert(result.diagnostics.windowResets.some((reset) => reset.reason === "generation:1->2"));
});

test("an SSE event resets the quiet window", async () => {
  const zfb = await startFakeZfb();
  setTimeout(() => zfb.emit("page"), 200);
  const result = await waitForDevSettled(zfb.baseUrl, { ...fast, timeoutMs: 5000 });
  assert(result.elapsedMs >= 200 + fast.quietMs, `settled too early: ${result.elapsedMs}ms`);
  assert.deepEqual(result.diagnostics.events.map((event) => event.event), ["page"]);
  assert(result.diagnostics.windowResets.some((reset) => reset.reason === "sse:page"));
});

test("not-ready keeps the window open until ready flips", async () => {
  const zfb = await startFakeZfb({ ready: false });
  setTimeout(() => {
    zfb.state.ready = true;
  }, 200);
  const result = await waitForDevSettled(zfb.baseUrl, { ...fast, timeoutMs: 5000 });
  // The window restarts at the last not-ready poll, at most one poll before the flip.
  assert(result.elapsedMs >= 200 - 2 * fast.pollMs + fast.quietMs, `settled too early: ${result.elapsedMs}ms`);
  assert(result.diagnostics.windowResets.some((reset) => reset.reason === "not-ready"));
});

test("falls back when /__zfb/ready is 404", async () => {
  const zfb = await startFakeZfb({ readyStatus: 404 });
  const result = await waitForDevSettled(zfb.baseUrl, { ...fast, timeoutMs: 5000 });
  assert.equal(result.diagnostics.readyEndpoint, "absent");
  assert(result.diagnostics.fallbacks.some((note) => note.includes("/__zfb/ready returned 404")));
});

test("falls back to SSE quiet when x-zfb-dev-generation is absent", async () => {
  const zfb = await startFakeZfb({ generation: null });
  setTimeout(() => zfb.emit("css"), 150);
  const result = await waitForDevSettled(zfb.baseUrl, { ...fast, timeoutMs: 5000 });
  assert.equal(result.generation, null);
  assert(result.elapsedMs >= 150 + fast.quietMs);
  assert(result.diagnostics.fallbacks.some((note) => note.includes("x-zfb-dev-generation absent")));
});

test("an SSE disconnect resubscribes and restarts the window", async () => {
  const zfb = await startFakeZfb();
  setTimeout(() => zfb.dropSse(), 150);
  const result = await waitForDevSettled(zfb.baseUrl, { ...fast, timeoutMs: 5000 });
  assert.equal(result.diagnostics.resubscribes, 1);
  assert.equal(zfb.state.sseConnections, 2);
  assert(result.elapsedMs >= 150 + fast.quietMs);
  await waitFor(() => zfb.openSseClients() === 0);
});

test("gives up after maxResubscribes disconnects", async () => {
  const zfb = await startFakeZfb();
  const timer = setInterval(() => zfb.dropSse(), 60);
  try {
    await assert.rejects(
      waitForDevSettled(zfb.baseUrl, { ...fast, maxResubscribes: 2, timeoutMs: 5000 }),
      (error) => {
        assert.match(error.message, /disconnected .* more than 2 times|could not be \(re\)subscribed/);
        assert.match(error.message, /resubscribes: 3/);
        return true;
      },
    );
  } finally {
    clearInterval(timer);
  }
});

test("a refused SSE subscription is bounded by maxResubscribes", async () => {
  const zfb = await startFakeZfb();
  zfb.state.refuseSse = true;
  await assert.rejects(
    waitForDevSettled(zfb.baseUrl, { ...fast, maxResubscribes: 1, timeoutMs: 5000 }),
    /could not be \(re\)subscribed after 1 resubscribes[\s\S]*responded 503/,
  );
});

test("a timeout error carries the ready payload, generation and events", async () => {
  const zfb = await startFakeZfb({ ready: false, generation: "7" });
  const pinger = setInterval(() => zfb.emit("islands", "Foo"), 50);
  try {
    await assert.rejects(waitForDevSettled(zfb.baseUrl, { ...fast, timeoutMs: 400 }), (error) => {
      assert.match(error.message, /did not settle within 400ms/);
      assert.match(error.message, /last ready payload: \{"generation":7,"ready":false/);
      assert.match(error.message, /last generation: 7/);
      assert.match(error.message, /islands\(Foo\)/);
      assert.equal(error.diagnostics.lastReadyPayload.ready, false);
      return true;
    });
  } finally {
    clearInterval(pinger);
  }
  await waitFor(() => zfb.openSseClients() === 0);
});

async function runChild(source) {
  const child = spawn(process.execPath, ["--input-type=module", "-e", source], { stdio: ["ignore", "pipe", "pipe"] });
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (chunk) => (stdout += chunk));
  child.stderr.on("data", (chunk) => (stderr += chunk));
  const started = Date.now();
  const killer = setTimeout(() => child.kill("SIGKILL"), 8000);
  const code = await new Promise((done) => child.on("exit", (exitCode, signal) => done(exitCode ?? signal)));
  clearTimeout(killer);
  return { code, stdout, stderr, ms: Date.now() - started };
}

test("no open handles remain after settle (child process exits on its own)", async () => {
  const zfb = await startFakeZfb();
  const child = await runChild(`
    import { waitForDevSettled } from ${JSON.stringify(harnessUrl)};
    await waitForDevSettled(${JSON.stringify(zfb.baseUrl)}, { quietMs: 200, pollMs: 25, timeoutMs: 5000 });
    console.log("settled");
  `);
  assert.equal(child.code, 0, child.stderr);
  assert.match(child.stdout, /settled/);
  assert(child.ms < 4000, `child lingered ${child.ms}ms`);
});

test("no open handles remain after timeout (child process exits on its own)", async () => {
  const zfb = await startFakeZfb({ ready: false });
  const child = await runChild(`
    import { waitForDevSettled } from ${JSON.stringify(harnessUrl)};
    try {
      await waitForDevSettled(${JSON.stringify(zfb.baseUrl)}, { quietMs: 200, pollMs: 25, timeoutMs: 300 });
    } catch (error) {
      console.log(error.message.split("\\n")[0]);
    }
  `);
  assert.equal(child.code, 0, child.stderr);
  assert.match(child.stdout, /did not settle within 300ms/);
  assert(child.ms < 4000, `child lingered ${child.ms}ms`);
});

test("SSE parser handles named events, comments, CRLF and split chunks", () => {
  const events = [];
  const parse = createSseParser((event) => events.push(event));
  parse(": keep-alive\n\nevent: pa");
  parse("ge\ndata: \n\r\nevent: islands\r\ndata: A\r\ndata: B\r\n\r\ndata: x\n\n");
  assert.deepEqual(events, [
    { event: "page", data: "" },
    { event: "islands", data: "A\nB" },
    { event: "message", data: "x" },
  ]);
});

// --- page-dependent helpers: pure parts with a minimal fake page ----------

class FakePage extends EventEmitter {
  constructor() {
    super();
    this.mainFrameObject = { url: () => "http://127.0.0.1/docs/" };
    this.initScripts = [];
    this.evaluateImpl = (fn, arg) => fn(arg);
  }
  mainFrame() {
    return this.mainFrameObject;
  }
  async addInitScript(fn, arg) {
    this.initScripts.push({ fn, arg });
  }
  async evaluate(fn, arg) {
    return this.evaluateImpl(fn, arg);
  }
  navigationRequest(url, frame = this.mainFrameObject) {
    this.emit("request", { isNavigationRequest: () => true, frame: () => frame, url: () => url });
  }
}

test("compareDocumentCheckpoint flags new document requests and identity changes", () => {
  assert.deepEqual(compareDocumentCheckpoint({ docId: "a", documentRequests: 1 }, { docId: "a", documentRequests: 1 }), []);
  const reasons = compareDocumentCheckpoint(
    { docId: "a", documentRequests: 1 },
    { docId: "b", documentRequests: 2, requestUrls: ["http://x/docs/"] },
  );
  assert.equal(reasons.length, 2);
  assert.match(reasons[0], /1 main-frame document request\(s\).*http:\/\/x\/docs\//);
  assert.match(reasons[1], /a -> b/);
});

test("plantDocumentId plants once per document", () => {
  const name = "__harnessTestDocId";
  try {
    plantDocumentId(name);
    const first = globalThis[name];
    assert.equal(typeof first, "string");
    plantDocumentId(name);
    assert.equal(globalThis[name], first);
  } finally {
    delete globalThis[name];
  }
});

test("guardDocumentReloads ignores same-document navigation and catches replacement", async () => {
  const page = new FakePage();
  let docId = "doc-1";
  page.evaluateImpl = () => docId;
  const dumps = [];
  const guard = await guardDocumentReloads(page, { diagnostics: { dump: async () => (dumps.push(1), "DIAG-DUMP") } });
  assert.equal(page.initScripts.length, 1);
  assert.equal(page.initScripts[0].fn, plantDocumentId);

  await guard.checkpoint("router");
  page.emit("framenavigated", page.mainFrame());
  page.navigationRequest("http://x/iframe", { url: () => "child" });
  page.emit("request", { isNavigationRequest: () => false, frame: () => page.mainFrame(), url: () => "http://x/a.js" });
  await guard.assertNoReloadSince("router");

  page.navigationRequest("http://x/docs/");
  docId = "doc-2";
  await assert.rejects(guard.assertNoReloadSince("router"), (error) => {
    assert.match(error.message, /^unexpected document reload during router: /);
    assert.match(error.message, /document identity changed \(doc-1 -> doc-2\)/);
    assert.match(error.message, /DIAG-DUMP/);
    return true;
  });
  assert.equal(dumps.length, 1);
  guard.dispose();
  assert.equal(page.listenerCount("request"), 0);
});

test("guardDocumentReloads treats a destroyed context during the check as a reload", async () => {
  const page = new FakePage();
  const guard = await guardDocumentReloads(page);
  page.evaluateImpl = () => "doc-1";
  await guard.checkpoint("swap");
  page.evaluateImpl = () => {
    throw new Error("page.evaluate: Execution context was destroyed, most likely because of a navigation");
  };
  await assert.rejects(guard.assertNoReloadSince("swap"), /unexpected document reload during swap: .*execution context destroyed/);
  await assert.rejects(guard.assertNoReloadSince("missing"), /no checkpoint named missing/);
});

test("probePlantedState reports presence in a single read", () => {
  const name = "__harnessTestPlanted";
  assert.deepEqual(probePlantedState(name), { present: false });
  globalThis[name] = undefined;
  assert.deepEqual(probePlantedState(name), { present: false });
  globalThis[name] = { token: 3 };
  try {
    assert.deepEqual(probePlantedState(name), { present: true, value: { token: 3 } });
  } finally {
    delete globalThis[name];
  }
});

test("readPlantedState returns absence, and rethrows destroyed contexts with diagnostics", async () => {
  const page = new FakePage();
  assert.deepEqual(await readPlantedState(page, "__harnessTestNope"), { present: false });

  page.evaluateImpl = () => {
    throw new Error("Execution context was destroyed, most likely because of a navigation");
  };
  await assert.rejects(readPlantedState(page, "__x", { diagnostics: { dump: async () => "DIAG" } }), (error) => {
    assert.equal(error.contextDestroyed, true);
    assert.match(error.message, /window\.__x .*execution context was destroyed[\s\S]*DIAG/);
    assert(error.cause);
    return true;
  });

  const unrelated = new Error("boom");
  page.evaluateImpl = () => {
    throw unrelated;
  };
  await assert.rejects(readPlantedState(page, "__x"), (error) => error === unrelated);
  assert.equal(isContextDestroyedError(unrelated), false);
});

test("formatDiagnostics merges node entries and page attribute log by time", () => {
  const text = formatDiagnostics(
    [
      { t: 1000, kind: "document-request", detail: "http://x/docs/" },
      { t: 3000, kind: "sse", detail: "page" },
    ],
    [{ t: 2000, name: "data-ccresdoc-load-controls-ready", value: "", doc: "abcdef1234" }],
  );
  const lines = text.split("\n");
  assert.match(lines[1], /document-request http:\/\/x\/docs\//);
  assert.match(lines[2], /attr data-ccresdoc-load-controls-ready="" doc=abcdef12/);
  assert.match(lines[3], /sse page/);
  assert.match(formatDiagnostics([], [], { attributeLogError: "gone" }), /page attribute log unavailable: gone/);
});

test("recordDiagnostics captures page events, SSE events and disposes cleanly", async () => {
  const zfb = await startFakeZfb();
  const page = new FakePage();
  page.evaluateImpl = () => [{ t: Date.now(), name: "data-ccresdoc-load-controls-ready", value: "" }];
  const recorder = await recordDiagnostics(page, zfb.baseUrl);
  assert.equal(page.initScripts[0].fn, installAttributeRecorder);
  await waitFor(() => zfb.openSseClients() === 1);
  await waitFor(() => recorder.entries.some((entry) => entry.detail === "subscribed"));

  page.navigationRequest("http://x/docs/");
  page.emit("framenavigated", page.mainFrame());
  page.emit("framenavigated", page.mainFrame());
  page.emit("console", { type: () => "error", text: () => "bad thing" });
  page.emit("console", { type: () => "log", text: () => "ignored" });
  page.emit("pageerror", new Error("thrown"));
  zfb.emit("page");
  await waitFor(() => recorder.entries.some((entry) => entry.kind === "sse" && entry.detail === "page"));

  const text = await recorder.dump();
  assert.match(text, /framenavigated .* sameDocument=false/);
  assert.match(text, /framenavigated .* sameDocument=true/);
  assert.match(text, /console-error bad thing/);
  assert.doesNotMatch(text, /ignored/);
  assert.match(text, /pageerror Error: thrown/);
  assert.match(text, /attr data-ccresdoc-load-controls-ready/);

  await recorder.dispose();
  for (const event of ["request", "framenavigated", "console", "pageerror"]) assert.equal(page.listenerCount(event), 0);
  await waitFor(() => zfb.openSseClients() === 0);
});

test("installAttributeRecorder records initial and mutated data-ccresdoc-* attributes", () => {
  const saved = { document: globalThis.document, MutationObserver: globalThis.MutationObserver };
  const observers = [];
  const attrs = new Map([["data-ccresdoc-load-controls-ready", ""], ["lang", "en"]]);
  const root = {
    get attributes() {
      return [...attrs].map(([name, value]) => ({ name, value }));
    },
    getAttribute: (name) => (attrs.has(name) ? attrs.get(name) : null),
  };
  globalThis.document = { documentElement: root };
  globalThis.MutationObserver = class {
    constructor(callback) {
      this.callback = callback;
      observers.push(this);
    }
    observe(target, options) {
      this.target = target;
      this.options = options;
    }
  };
  const bufferName = "__harnessTestAttrLog";
  try {
    installAttributeRecorder({ bufferName, prefix: "data-ccresdoc-", docIdName: "__harnessTestDoc" });
    const buffer = globalThis[bufferName];
    assert.equal(buffer.length, 1);
    assert.equal(buffer[0].initial, true);
    assert.equal(observers[0].target, root);
    assert.deepEqual(observers[0].options, { attributes: true });

    attrs.delete("data-ccresdoc-load-controls-ready");
    observers[0].callback([{ attributeName: "data-ccresdoc-load-controls-ready" }, { attributeName: "data-theme" }]);
    assert.equal(buffer.length, 2);
    assert.equal(buffer[1].value, null);
  } finally {
    delete globalThis[bufferName];
    globalThis.document = saved.document;
    globalThis.MutationObserver = saved.MutationObserver;
  }
});
