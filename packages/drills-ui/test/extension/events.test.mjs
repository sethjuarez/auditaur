import { test } from "node:test";
import assert from "node:assert/strict";
import { lib } from "./lib.mjs";

const { foldEvents } = await lib("events.mjs");

const lines = (...events) => events.map((event) => JSON.stringify(event)).join("\n");
const hook = { suiteId: "s", testId: null, attempt: null, index: 0 };

test("a running step shows its latest progress, and drops it when it finishes", () => {
    const started = { ...hook, type: "stepStarted", phase: "beforeAll", title: "launch app", action: "launch", ts: "t" };
    const progress = (message) => ({ ...hook, type: "stepProgress", message });
    const running = foldEvents(lines(started, progress("Compiling tauri"), progress("Building 410/520")));
    assert.equal(running.steps.get("s||")[0].progress, "Building 410/520");

    const done = foldEvents(lines(started, progress("Compiling tauri"), { ...hook, type: "stepFinished", status: "passed", soft: false, durationMs: 5 }));
    assert.equal(done.steps.get("s||")[0].progress, null);
});
