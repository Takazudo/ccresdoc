// Shared helpers for the browser harnesses (confirm-browser-navigation.mjs,
// confirm-hydration-readiness.mjs): a dev-server settle wait, a document
// replacement guard, and a diagnostics recorder. Nothing here stubs or
// disables /__zfb/livereload.js, retries a failed check, or defaults missing
// window state — the helpers only observe and report.
//
// zfb dev surfaces (external contract, confirmed against the installed
// @takazudo/zfb native binary, 2.22.x):
// - GET /__zfb/ready -> JSON `{ generation: number, ready: boolean,
//   islands: { status }, client_scripts: { status }, documents:
//   "pending" | "published" | "ready_on_request" | "not_expected",
//   exclusions: {...} }`. `ready === true` is the deciding field.
// - HTML page responses carry `x-zfb-dev-generation: <n>` and
//   `x-zfb-dev-ready: true|false` (non-HTML responses carry neither).
// - GET /__zfb/reload is an SSE stream with named events `page`, `css`,
//   `islands` (data may be empty) plus `:` keep-alive comments every 15s.

export const RELOAD_EVENT_NAMES = Object.freeze(["page", "css", "islands"]);
export const READINESS_ATTRIBUTE_PREFIX = "data-ccresdoc-";
const DOC_ID_GLOBAL = "__ccresdocDocId";
const ATTR_LOG_GLOBAL = "__ccresdocAttrLog";

function joinUrl(baseUrl, path) {
  return new URL(path, baseUrl.endsWith("/") ? baseUrl : `${baseUrl}/`).toString();
}

function abortError(signal) {
  return signal.reason instanceof Error ? signal.reason : new Error("aborted");
}

function sleep(ms, signal) {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) return reject(abortError(signal));
    const onAbort = () => {
      clearTimeout(timer);
      reject(abortError(signal));
    };
    const timer = setTimeout(() => {
      signal?.removeEventListener("abort", onAbort);
      resolve();
    }, ms);
    signal?.addEventListener("abort", onAbort, { once: true });
  });
}

// Fetch bounded by `timeoutMs` and by an optional outer signal. The body is
// read (or cancelled) inside the bound so no socket outlives the call.
async function boundedFetch(url, { timeoutMs, signal, readBody }) {
  const controller = new AbortController();
  const timer = setTimeout(
    () => controller.abort(new Error(`fetch ${url} timed out after ${timeoutMs}ms`)),
    timeoutMs,
  );
  const onOuterAbort = () => controller.abort(abortError(signal));
  if (signal?.aborted) onOuterAbort();
  else signal?.addEventListener("abort", onOuterAbort, { once: true });
  try {
    const response = await fetch(url, { signal: controller.signal });
    let body = null;
    if (readBody) body = await response.text();
    else await response.body?.cancel().catch(() => {});
    return { status: response.status, headers: response.headers, body };
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener("abort", onOuterAbort);
  }
}

// Incremental parser for a text/event-stream body. Feed decoded text chunks;
// each completed event is passed to `onEvent({ event, data })`.
export function createSseParser(onEvent) {
  let buffer = "";
  let event = "";
  let data = [];
  const dispatch = () => {
    if (event || data.length > 0) onEvent({ event: event || "message", data: data.join("\n") });
    event = "";
    data = [];
  };
  return (chunk) => {
    buffer += chunk;
    let newline;
    while ((newline = buffer.search(/\r\n|\r|\n/)) !== -1) {
      const line = buffer.slice(0, newline);
      buffer = buffer.slice(newline + (buffer.startsWith("\r\n", newline) ? 2 : 1));
      if (line === "") dispatch();
      else if (line.startsWith(":")) continue;
      else {
        const colon = line.indexOf(":");
        const field = colon === -1 ? line : line.slice(0, colon);
        const value = colon === -1 ? "" : line.slice(colon + 1).replace(/^ /, "");
        if (field === "event") event = value;
        else if (field === "data") data.push(value);
      }
    }
  };
}

// Opens GET <base>/__zfb/reload from Node. `live` resolves once the response
// headers arrived (subscription is active); `closed` resolves with the reason
// the stream ended. Establishing the stream is bounded by `connectTimeoutMs`.
export function openReloadStream(baseUrl, { onEvent, connectTimeoutMs = 5000 }) {
  const url = joinUrl(baseUrl, "__zfb/reload");
  const controller = new AbortController();
  let closedByCaller = false;
  let resolveLive;
  let rejectLive;
  const live = new Promise((resolve, reject) => {
    resolveLive = resolve;
    rejectLive = reject;
  });
  live.catch(() => {});
  const connectTimer = setTimeout(
    () => controller.abort(new Error(`SSE ${url} did not connect within ${connectTimeoutMs}ms`)),
    connectTimeoutMs,
  );
  const closed = (async () => {
    try {
      const response = await fetch(url, {
        signal: controller.signal,
        headers: { accept: "text/event-stream" },
      });
      clearTimeout(connectTimer);
      if (!response.ok || !response.body) {
        await response.body?.cancel().catch(() => {});
        throw new Error(`SSE ${url} responded ${response.status}`);
      }
      resolveLive();
      const parse = createSseParser((event) => onEvent?.(event));
      const decoder = new TextDecoder();
      for await (const chunk of response.body) parse(decoder.decode(chunk, { stream: true }));
      return { reason: "ended", byCaller: closedByCaller };
    } catch (error) {
      rejectLive(error);
      return { reason: closedByCaller ? "closed" : String(error?.message ?? error), byCaller: closedByCaller, error };
    } finally {
      clearTimeout(connectTimer);
    }
  })();
  return {
    live,
    closed,
    close() {
      closedByCaller = true;
      controller.abort(new Error("SSE closed by caller"));
      return closed;
    },
  };
}

function describeSettleDiagnostics(diag) {
  return [
    `ready endpoint: ${diag.readyEndpoint}`,
    `last ready payload: ${diag.lastReadyPayload === undefined ? "(none)" : JSON.stringify(diag.lastReadyPayload)}`,
    `last generation: ${diag.lastGeneration ?? "(none)"}`,
    `fallbacks: ${diag.fallbacks.length ? diag.fallbacks.join("; ") : "(none)"}`,
    `resubscribes: ${diag.resubscribes}`,
    `window resets: ${diag.windowResets.length ? diag.windowResets.map((r) => `${r.atMs}ms ${r.reason}`).join(", ") : "(none)"}`,
    `observed SSE events: ${diag.events.length ? diag.events.map((e) => `${e.atMs}ms ${e.event}${e.data ? `(${e.data})` : ""}`).join(", ") : "(none)"}`,
  ].join("\n");
}

// Resolves once the zfb dev server has been quiet for `quietMs`: ready
// endpoint true (unless absent), `x-zfb-dev-generation` on /docs/ unchanged
// (unless absent), and no /__zfb/reload event. Throws with diagnostics on
// timeout or after `maxResubscribes` SSE disconnects. Always aborts the
// stream and clears its timers before returning, so the process can exit.
export async function waitForDevSettled(
  baseUrl,
  { quietMs = 1500, timeoutMs = 60000, fetchTimeoutMs = 5000, maxResubscribes = 3, pollMs, probePath = "/docs/" } = {},
) {
  const startedAt = Date.now();
  const elapsed = () => Date.now() - startedAt;
  const interval = pollMs ?? Math.max(25, Math.min(250, Math.floor(quietMs / 4)));
  const diag = {
    readyEndpoint: "unknown",
    lastReadyPayload: undefined,
    lastGeneration: undefined,
    fallbacks: [],
    resubscribes: 0,
    windowResets: [],
    events: [],
  };
  const fail = (message, cause) => {
    const causeLine = cause ? `\ncause: ${String(cause?.message ?? cause)}` : "";
    const error = new Error(`${message}${causeLine}\n${describeSettleDiagnostics(diag)}`, cause ? { cause } : undefined);
    error.diagnostics = diag;
    return error;
  };

  const master = new AbortController();
  const deadline = setTimeout(() => master.abort(new Error("settle timeout")), timeoutMs);
  let stream = null;
  let streamEnded = null;
  let windowStart = 0;
  const resetWindow = (reason) => {
    windowStart = Date.now();
    diag.windowResets.push({ atMs: elapsed(), reason });
  };

  const subscribe = async () => {
    const current = openReloadStream(baseUrl, {
      connectTimeoutMs: fetchTimeoutMs,
      onEvent: ({ event, data }) => {
        diag.events.push({ atMs: elapsed(), event, data });
        resetWindow(`sse:${event}`);
      },
    });
    stream = current;
    streamEnded = null;
    current.closed.then((result) => {
      if (stream === current && !result.byCaller) streamEnded = result.reason;
    });
    const onAbort = () => current.close();
    master.signal.addEventListener("abort", onAbort, { once: true });
    current.closed.then(() => master.signal.removeEventListener("abort", onAbort));
    await current.live;
  };

  const subscribeOrResubscribe = async (reason) => {
    for (;;) {
      try {
        await subscribe();
        resetWindow(reason);
        return;
      } catch (error) {
        if (master.signal.aborted) throw error;
        diag.resubscribes += 1;
        if (diag.resubscribes > maxResubscribes) {
          throw fail(`/__zfb/reload could not be (re)subscribed after ${maxResubscribes} resubscribes`, error);
        }
        reason = `sse-resubscribe:${String(error?.message ?? error)}`;
      }
    }
  };

  const probeReady = async () => {
    if (diag.readyEndpoint === "absent") return true;
    const res = await boundedFetch(joinUrl(baseUrl, "__zfb/ready"), {
      timeoutMs: fetchTimeoutMs,
      signal: master.signal,
      readBody: true,
    });
    if (res.status === 404) {
      diag.readyEndpoint = "absent";
      diag.fallbacks.push("/__zfb/ready returned 404; settling on generation + SSE quiet only");
      return true;
    }
    diag.readyEndpoint = "present";
    let payload;
    try {
      payload = JSON.parse(res.body);
    } catch {
      payload = { unparsable: res.body?.slice(0, 200), status: res.status };
    }
    diag.lastReadyPayload = payload;
    return res.status === 200 && payload?.ready === true;
  };

  let headerFallbackRecorded = false;
  const probeGeneration = async () => {
    const res = await boundedFetch(joinUrl(baseUrl, probePath.replace(/^\//, "")), {
      timeoutMs: fetchTimeoutMs,
      signal: master.signal,
      readBody: false,
    });
    const generation = res.headers.get("x-zfb-dev-generation");
    if (generation === null && !headerFallbackRecorded) {
      headerFallbackRecorded = true;
      diag.fallbacks.push(`x-zfb-dev-generation absent on ${probePath}; settling on SSE quiet only`);
    }
    return generation;
  };

  try {
    await subscribeOrResubscribe("sse-live");
    let windowGeneration;
    for (;;) {
      if (streamEnded !== null) {
        const reason = streamEnded;
        diag.resubscribes += 1;
        if (diag.resubscribes > maxResubscribes) {
          throw fail(`/__zfb/reload disconnected (${reason}) more than ${maxResubscribes} times`);
        }
        await subscribeOrResubscribe(`sse-resubscribe:${reason}`);
        windowGeneration = undefined;
      }
      let probesOk = true;
      let ready = false;
      try {
        ready = await probeReady();
        if (!ready) resetWindow("not-ready");
        const generation = await probeGeneration();
        if (generation !== null) diag.lastGeneration = generation;
        if (windowGeneration === undefined) windowGeneration = generation;
        else if (generation !== windowGeneration) {
          resetWindow(`generation:${windowGeneration}->${generation}`);
          windowGeneration = generation;
        }
      } catch (error) {
        if (master.signal.aborted) throw error;
        // A slow or failed probe is not evidence of quiet: restart the window
        // and keep polling until the overall deadline.
        probesOk = false;
        resetWindow(`probe-failed:${String(error?.message ?? error)}`);
      }
      if (probesOk && ready && streamEnded === null && Date.now() - windowStart >= quietMs) {
        return {
          elapsedMs: elapsed(),
          generation: diag.lastGeneration ?? null,
          readyPayload: diag.lastReadyPayload ?? null,
          diagnostics: diag,
        };
      }
      await sleep(interval, master.signal);
    }
  } catch (error) {
    if (master.signal.aborted && master.signal.reason?.message === "settle timeout") {
      throw fail(`zfb dev server did not settle within ${timeoutMs}ms (quietMs=${quietMs})`, error);
    }
    if (error?.diagnostics) throw error;
    throw fail(`waitForDevSettled failed: ${String(error?.message ?? error)}`, error);
  } finally {
    clearTimeout(deadline);
    if (!master.signal.aborted) master.abort(new Error("settled"));
    if (stream) await stream.close();
  }
}

// --- document replacement guard -------------------------------------------

export function isContextDestroyedError(error) {
  return /Execution context was destroyed|Cannot find context with specified id|most likely because of a navigation|Frame was detached|Target (page, context or browser )?(has been )?closed/i
    .test(String(error?.message ?? error));
}

// Page-side identity planter, registered with page.addInitScript so each new
// document (not same-document router navigation) gets a fresh token.
export function plantDocumentId(globalName) {
  const w = globalThis;
  if (w[globalName]) return;
  const id = w.crypto?.randomUUID?.() ?? `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  Object.defineProperty(w, globalName, { value: id, configurable: true });
}

// Pure comparison between a checkpoint and the current observation. Returns
// the list of reasons a document replacement is evident (empty = none).
export function compareDocumentCheckpoint(before, after) {
  const reasons = [];
  if (after.documentRequests > before.documentRequests) {
    reasons.push(
      `${after.documentRequests - before.documentRequests} main-frame document request(s) since checkpoint` +
        (after.requestUrls?.length ? `: ${after.requestUrls.join(", ")}` : ""),
    );
  }
  if (after.docId !== before.docId) {
    reasons.push(`document identity changed (${before.docId ?? "none"} -> ${after.docId ?? "none"})`);
  }
  return reasons;
}

// Detects document replacement between checkpoints. Call before the first
// page.goto so the identity token is planted in every document.
export async function guardDocumentReloads(page, { diagnostics } = {}) {
  const documentRequests = [];
  const checkpoints = new Map();
  const onRequest = (request) => {
    if (request.isNavigationRequest() && request.frame() === page.mainFrame()) {
      documentRequests.push({ atMs: Date.now(), url: request.url() });
    }
  };
  page.on("request", onRequest);
  await page.addInitScript(plantDocumentId, DOC_ID_GLOBAL);

  const readDocId = async () => {
    try {
      return { docId: await page.evaluate((name) => globalThis[name] ?? null, DOC_ID_GLOBAL) };
    } catch (error) {
      if (isContextDestroyedError(error)) return { docId: null, readError: String(error?.message ?? error) };
      throw error;
    }
  };

  return {
    documentRequests,
    async checkpoint(label) {
      const { docId, readError } = await readDocId();
      if (readError) throw new Error(`checkpoint ${label}: document identity unreadable (${readError})`);
      checkpoints.set(label, { docId, documentRequests: documentRequests.length });
    },
    async assertNoReloadSince(label) {
      const before = checkpoints.get(label);
      if (!before) throw new Error(`no checkpoint named ${label}`);
      const { docId, readError } = await readDocId();
      const after = {
        docId,
        documentRequests: documentRequests.length,
        requestUrls: documentRequests.slice(before.documentRequests).map((entry) => entry.url),
      };
      const reasons = compareDocumentCheckpoint(before, after);
      if (readError) reasons.push(`execution context destroyed while reading identity: ${readError}`);
      if (reasons.length === 0) return;
      const dump = diagnostics ? `\n${await diagnostics.dump()}` : "";
      throw new Error(`unexpected document reload during ${label}: ${reasons.join("; ")}${dump}`);
    },
    dispose() {
      page.off("request", onRequest);
    },
  };
}

// --- diagnostics recorder --------------------------------------------------

// Page-side readiness recorder, registered with page.addInitScript. A
// MutationObserver on <html> attributes pushes timestamped changes of
// data-ccresdoc-* attributes into a page buffer; nothing polls.
export function installAttributeRecorder({ bufferName, prefix, docIdName }) {
  const w = globalThis;
  if (w[bufferName]) return;
  const buffer = [];
  Object.defineProperty(w, bufferName, { value: buffer, configurable: true });
  const stamp = () => Math.round(performance.timeOrigin + performance.now());
  const observe = (root) => {
    for (const attr of Array.from(root.attributes)) {
      if (attr.name.startsWith(prefix)) buffer.push({ t: stamp(), name: attr.name, value: attr.value, initial: true, doc: w[docIdName] ?? null });
    }
    new MutationObserver((records) => {
      for (const record of records) {
        if (!record.attributeName?.startsWith(prefix)) continue;
        buffer.push({ t: stamp(), name: record.attributeName, value: root.getAttribute(record.attributeName), doc: w[docIdName] ?? null });
      }
    }).observe(root, { attributes: true });
  };
  if (document.documentElement) observe(document.documentElement);
  else {
    const waiter = new MutationObserver(() => {
      if (!document.documentElement) return;
      waiter.disconnect();
      observe(document.documentElement);
    });
    waiter.observe(document, { childList: true });
  }
}

function iso(t) {
  return new Date(t).toISOString().slice(11, 23);
}

// Pure formatter: Node-side entries `{ t, kind, detail }` plus the page-side
// attribute log, merged by timestamp into a compact text block.
export function formatDiagnostics(entries, attributeLog = [], { attributeLogError } = {}) {
  const rows = [
    ...entries.map((entry) => ({ t: entry.t, line: `${entry.kind} ${entry.detail}` })),
    ...attributeLog.map((entry) => ({
      t: entry.t,
      line: `attr ${entry.name}=${entry.value === null ? "(removed)" : JSON.stringify(entry.value)}${entry.initial ? " (initial)" : ""}${entry.doc ? ` doc=${String(entry.doc).slice(0, 8)}` : ""}`,
    })),
  ].sort((a, b) => a.t - b.t);
  const lines = ["--- browser harness diagnostics ---", ...rows.map((row) => `${iso(row.t)} ${row.line}`)];
  if (attributeLogError) lines.push(`(page attribute log unavailable: ${attributeLogError})`);
  if (rows.length === 0 && !attributeLogError) lines.push("(no events recorded)");
  lines.push("--- end diagnostics ---");
  return lines.join("\n");
}

export async function recordDiagnostics(page, baseUrl, { subscribeReload = true, connectTimeoutMs = 5000 } = {}) {
  const entries = [];
  const push = (kind, detail) => entries.push({ t: Date.now(), kind, detail });
  let documentRequestsSinceNavigated = 0;
  const onRequest = (request) => {
    if (request.isNavigationRequest() && request.frame() === page.mainFrame()) {
      documentRequestsSinceNavigated += 1;
      push("document-request", request.url());
    }
  };
  const onFrameNavigated = (frame) => {
    if (frame !== page.mainFrame()) return;
    // Playwright exposes no same-document flag; infer it from whether a
    // main-frame document request preceded this event.
    const sameDocument = documentRequestsSinceNavigated === 0;
    documentRequestsSinceNavigated = 0;
    push("framenavigated", `${frame.url()} sameDocument=${sameDocument} (inferred)`);
  };
  const onConsole = (message) => {
    if (message.type() === "error") push("console-error", message.text());
  };
  const onPageError = (error) => push("pageerror", String(error?.stack ?? error?.message ?? error));
  page.on("request", onRequest);
  page.on("framenavigated", onFrameNavigated);
  page.on("console", onConsole);
  page.on("pageerror", onPageError);
  await page.addInitScript(installAttributeRecorder, {
    bufferName: ATTR_LOG_GLOBAL,
    prefix: READINESS_ATTRIBUTE_PREFIX,
    docIdName: DOC_ID_GLOBAL,
  });

  let stream = null;
  if (subscribeReload && baseUrl) {
    stream = openReloadStream(baseUrl, {
      connectTimeoutMs,
      onEvent: ({ event, data }) => push("sse", `${event}${data ? ` ${data}` : ""}`),
    });
    stream.live.then(() => push("sse", "subscribed"), (error) => push("sse", `subscribe failed: ${error?.message ?? error}`));
    stream.closed.then((result) => {
      if (!result.byCaller) push("sse", `stream closed: ${result.reason}`);
    });
  }

  return {
    entries,
    async dump() {
      let attributeLog = [];
      let attributeLogError;
      try {
        attributeLog = (await page.evaluate((name) => globalThis[name] ?? [], ATTR_LOG_GLOBAL)) ?? [];
      } catch (error) {
        attributeLogError = String(error?.message ?? error);
      }
      return formatDiagnostics(entries, attributeLog, { attributeLogError });
    },
    async dispose() {
      page.off("request", onRequest);
      page.off("framenavigated", onFrameNavigated);
      page.off("console", onConsole);
      page.off("pageerror", onPageError);
      if (stream) await stream.close();
    },
  };
}

// --- planted window state ---------------------------------------------------

// Page-side probe: presence and value read in one evaluate, so a document
// swap cannot land between a Node-side presence check and the value read.
export function probePlantedState(globalName) {
  const w = globalThis;
  if (!(globalName in w) || w[globalName] === undefined) return { present: false };
  return { present: true, value: w[globalName] };
}

// Returns `{ present: false }` when the global is absent (callers turn that
// into a failure carrying assertNoReloadSince + dump()). An evaluate that
// fails because the execution context was destroyed is rethrown with the
// diagnostics attached.
export async function readPlantedState(page, globalName, { diagnostics } = {}) {
  try {
    return await page.evaluate(probePlantedState, globalName);
  } catch (error) {
    if (!isContextDestroyedError(error)) throw error;
    const dump = diagnostics ? `\n${await diagnostics.dump()}` : "";
    const wrapped = new Error(
      `reading window.${globalName} failed because the execution context was destroyed (document replaced?): ${String(error?.message ?? error)}${dump}`,
      { cause: error },
    );
    wrapped.contextDestroyed = true;
    throw wrapped;
  }
}
