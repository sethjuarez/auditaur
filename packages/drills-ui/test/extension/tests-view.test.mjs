import { test } from "node:test";
import assert from "node:assert/strict";
import { LIB } from "./lib.mjs";

const { buildView } = await import(new URL("../features/tests/view.mjs", LIB));

const state = (suites) => ({ view: { get: () => ({ filter: "" }) }, list: { suites, invalid: [] }, workspace: "w" });
const run = (runId, status, outcome) => ({
    runId,
    status: "failed",
    totals: { tests: 1, passed: 0, skipped: 1 },
    suites: [{ suite: "auth", tests: [{ id: "t", status, outcome, durationMs: 80_700 }] }],
});

test("a test skipped mid-run reads as skipped, not as its duration alone", () => {
    const view = buildView(state([{ suite: "auth", tests: [{ id: "t", name: "t", title: "auth › t" }] }]), [run("r2", "skipped", "skipped")]);
    assert.deepEqual(view.suites[0].tests[0].last, { runId: "r2", result: "skipped", durationMs: 80_700, error: null });
});

test("each run lists its suite names", () => {
    const view = buildView(state([]), [run("r1", "passed", "expected")]);
    assert.deepEqual(view.runs[0].suites, ["auth"]);
    assert.equal(view.runs[0].totals.skipped, 1);
});

test("the active run's pending gates reach the Tests page, trimmed to what the banner shows", () => {
    const active = { ...run("r3", "running", null), status: "running" };
    const gate = { requestId: "q", test: "auth › t", name: "Enter code", since: "2026-10-30T00:00:00Z", timeoutMs: 600_000, instructions: "x", choices: [] };
    const view = buildView(state([]), [active], [gate]);
    assert.deepEqual(view.active.gates, [{ test: "auth › t", name: "Enter code", since: "2026-10-30T00:00:00Z", timeoutMs: 600_000 }]);
});
