// Incremental parser for a text/event-stream body (WHATWG HTML "Server-sent
// events" interpretation). Feed decoded text chunks in arrival order; a frame
// is dispatched to `onEvent({ event, data })` only once its terminating blank
// line has arrived. Lines end in CRLF, LF or a lone CR. A CR that ends one
// chunk is a complete line ending, and an LF that starts the next chunk is
// the second half of that same CRLF, not an extra blank line.
//
// `end()` reports the stream's end. Per the spec, a frame without its blank
// line is discarded rather than dispatched; `end()` returns whether such a
// partial frame (or an unterminated line) was pending.
export function createSseParser(onEvent) {
  let buffer = "";
  let event = "";
  let data = [];
  let hasData = false;
  let pendingCr = false;

  const dispatch = () => {
    if (hasData) onEvent({ event: event || "message", data: data.join("\n") });
    event = "";
    data = [];
    hasData = false;
  };

  const processLine = (line) => {
    if (line === "") {
      dispatch();
      return;
    }
    if (line.startsWith(":")) return;
    const colon = line.indexOf(":");
    const field = colon === -1 ? line : line.slice(0, colon);
    const value = colon === -1 ? "" : line.slice(colon + 1).replace(/^ /, "");
    if (field === "event") event = value;
    else if (field === "data") {
      data.push(value);
      hasData = true;
    }
  };

  const feed = (chunk) => {
    let text = String(chunk);
    if (text === "") return;
    if (pendingCr) {
      pendingCr = false;
      if (text.startsWith("\n")) text = text.slice(1);
    }
    buffer += text;
    let start = 0;
    for (let index = 0; index < buffer.length; index += 1) {
      const char = buffer[index];
      if (char !== "\r" && char !== "\n") continue;
      processLine(buffer.slice(start, index));
      if (char === "\r") {
        if (index + 1 === buffer.length) pendingCr = true;
        else if (buffer[index + 1] === "\n") index += 1;
      }
      start = index + 1;
    }
    buffer = buffer.slice(start);
  };

  feed.end = () => {
    const partial = buffer.length > 0 || hasData || event !== "";
    buffer = "";
    event = "";
    data = [];
    hasData = false;
    pendingCr = false;
    return partial;
  };

  return feed;
}
