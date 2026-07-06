"use strict";
(() => {
  const hostCall = globalThis.__oceans_host_call;
  const hostLog = globalThis.__oceans_log;
  delete globalThis.__oceans_host_call;
  delete globalThis.__oceans_log;

  // Every host call completes synchronously from the sandbox's point of view:
  // the async wrappers below exist only so `await oceans.*()` reads naturally.
  // `{"error"}` envelopes become ordinary catchable exceptions.
  const call = (name, args) => {
    const raw = hostCall(name, JSON.stringify(args === undefined ? {} : args));
    const envelope = JSON.parse(raw);
    if (envelope !== null && typeof envelope === "object" && "error" in envelope) {
      throw new Error(String(envelope.error));
    }
    return envelope.result;
  };

  globalThis.oceans = Object.freeze({
    searchTools: async (args) => call("searchTools", args),
    describeTool: async (args) => call("describeTool", args),
    callTool: async (args) => call("callTool", args),
  });

  const format = (value) => {
    if (typeof value === "string") return value;
    try {
      const encoded = JSON.stringify(value);
      return encoded === undefined ? String(value) : encoded;
    } catch (_error) {
      return String(value);
    }
  };
  const emit = (prefix) => (...args) => {
    hostLog(prefix + args.map(format).join(" "));
  };
  globalThis.console = Object.freeze({
    log: emit(""),
    info: emit(""),
    debug: emit(""),
    warn: emit("[warn] "),
    error: emit("[error] "),
  });
})();
