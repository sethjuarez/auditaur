import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { LIB, tempDir } from "./lib.mjs";

const { traceView } = await import(new URL("../features/trace/view.mjs", LIB));
const { traceSummary } = await import(new URL("../features/trace/summary.mjs", LIB));
const { ViewState } = await import(new URL("view-state.mjs", LIB));
const { default: trace } = await import(new URL("../features/trace/index.mjs", LIB));

const RUN = "20261030-000000-000";
const T0 = Date.parse("2026-10-30T00:00:00.000Z");
const iso = (ms) => new Date(T0 + ms).toISOString();
const nanos = (ms) => String(BigInt(T0 + ms) * 1_000_000n);

async function workspace() {
    const dir = await tempDir("trace-view-");
    const runDir = join(dir, ".auditaur", "drill-runs", RUN);
    await mkdir(join(runDir, "tests", "ok", "attempt-0"), { recursive: true });
    await mkdir(join(runDir, "tests", "bad", "attempt-1"), { recursive: true });
    await writeFile(join(runDir, "run.json"), JSON.stringify({
        runId: RUN,
        status: "failed",
        startedAt: iso(0),
        totals: { passed: 1, failed: 1 },
        suites: [{
            id: "s",
            suite: "smoke",
            hooks: [{ title: "run seed", status: "passed" }],
            tests: [
                { id: "ok", title: "smoke › ok", status: "passed", outcome: "expected", attempts: [{ attempt: 0, status: "passed" }] },
                { id: "bad", title: "smoke › bad", status: "failed", outcome: "unexpected", attempts: [{ attempt: 0, status: "failed" }, { attempt: 1, status: "failed" }] },
            ],
        }],
    }));
    await writeFile(join(runDir, "tests", "bad", "attempt-1", "attempt.json"), JSON.stringify({
        status: "failed",
        startedAt: iso(0),
        durationMs: 300,
        error: { message: "boom" },
        steps: [
            { phase: "test", title: "click #go", action: "click", status: "passed", startedAt: iso(10), durationMs: 5, attachments: [] },
            { phase: "test", title: "expect #done visible", action: "expect", status: "failed", startedAt: iso(100), durationMs: 200, error: { message: "boom" }, attachments: [{ name: "failure", contentType: "image/png", path: "tests/bad/attempt-1/f.png" }] },
        ],
        attachments: [
            { name: "telemetry", contentType: "application/json", path: "tests/bad/attempt-1/telemetry.json" },
            { name: "dom", contentType: "text/html", path: "tests/bad/attempt-1/dom.html" },
        ],
    }));
    await writeFile(join(runDir, "tests", "bad", "attempt-1", "telemetry.json"), JSON.stringify({
        spans: [
            { name: "tauri.invoke save", startTimeUnixNanos: nanos(5), endTimeUnixNanos: nanos(6), attributes: { "tauri.command": "early", "tauri.command.args": "{}" } },
            { name: "tauri.invoke save", startTimeUnixNanos: nanos(20), endTimeUnixNanos: nanos(40), statusCode: "ok", attributes: { "tauri.command": "save", "tauri.command.args": "{\"id\":1}" } },
        ],
    }));
    return dir;
}

const stateFor = async (overrides = {}) => ({ workspace: await workspace(), runId: null, testId: null, attempt: null, view: new ViewState(trace.view), ...overrides });

test("selects the first failed test's last attempt and lines telemetry up with steps", async () => {
    const view = await traceView(await stateFor());
    assert.deepEqual(view.runs, [{ runId: RUN, status: "failed" }]);
    assert.deepEqual(view.run, { runId: RUN, status: "failed", startedAt: iso(0), totals: { passed: 1, failed: 1 } });
    assert.deepEqual(view.entries.map(({ key, kind, result, attempts }) => ({ key, kind, result, attempts: attempts.length })), [
        { key: "hooks:s", kind: "hooks", result: "passed", attempts: 0 },
        { key: "ok", kind: "test", result: "passed", attempts: 1 },
        { key: "bad", kind: "test", result: "failed", attempts: 2 },
    ]);
    const selected = view.selected;
    assert.equal(selected.key, "bad");
    assert.equal(selected.attempt, 1);
    assert.equal(selected.error, "boom");
    assert.equal(selected.step, 1);
    assert.deepEqual(selected.steps.map(({ offsetMs, error }) => ({ offsetMs, error })), [
        { offsetMs: 10, error: null },
        { offsetMs: 100, error: "boom" },
    ]);
    assert.deepEqual(selected.steps[1].attachments, [{ name: "failure", contentType: "image/png", path: "tests/bad/attempt-1/f.png" }]);
    assert.deepEqual(selected.attachments, [{ name: "dom", contentType: "text/html", path: "tests/bad/attempt-1/dom.html" }]);
    assert.deepEqual(selected.events.map(({ name, offsetMs, step }) => ({ name, offsetMs, step })), [
        { name: "early", offsetMs: 5, step: null },
        { name: "save", offsetMs: 20, step: 0 },
    ]);

    const summary = traceSummary(view);
    assert.equal(summary.selected.beforeFirstStep, 1);
    assert.deepEqual(summary.selected.steps[0].telemetry.map(({ name }) => name), ["save"]);
    assert.deepEqual(summary.tests.map(({ testId, attempts }) => ({ testId, attempts })), [
        { testId: "hooks:s", attempts: 0 },
        { testId: "ok", attempts: 1 },
        { testId: "bad", attempts: 2 },
    ]);
});

test("routes to hooks and to a missing attempt report", async () => {
    const hooks = await traceView(await stateFor({ testId: "hooks:s" }));
    assert.equal(hooks.selected.kind, "hooks");
    assert.equal(hooks.selected.attempt, null);
    assert.deepEqual(hooks.selected.steps.map(({ title }) => title), ["run seed"]);

    const missing = await traceView(await stateFor({ testId: "ok" }));
    assert.equal(missing.selected.missing, true);
    assert.equal(missing.selected.status, "passed");
    assert.equal(missing.selected.step, null);
});

test("a pinned step resets when another attempt comes into view", async () => {
    const state = await stateFor();
    await traceView(state);
    state.view.set({ step: 0 });
    assert.equal((await traceView(state)).selected.step, 0);
    state.attempt = 0;
    const moved = await traceView(state);
    assert.equal(moved.selected.attempt, 0);
    assert.equal(state.view.get().step, "auto");
});
