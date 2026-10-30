import { test } from "node:test";
import assert from "node:assert/strict";
import { LIB } from "./lib.mjs";

const { coverageSummary, baselineOf, runApps } = await import(new URL("../features/coverage/view.mjs", LIB));

const coverage = (sources, extra = {}) => ({ sources, totals: {}, commands: [], events: [], tests: [], ...extra });
const none = { commands: false, events: false };

test("without the CLI's baseline, both kinds have one only when a store could be read", () => {
    assert.deepEqual(baselineOf(coverage([])), none);
    assert.deepEqual(baselineOf(coverage([{ db: "a", error: "no telemetry store" }])), none);
    assert.deepEqual(baselineOf(coverage([{ db: "a", error: "x" }, { db: "b" }])), { commands: true, events: true });
    assert.deepEqual(coverageSummary({ run: null, coverage: coverage([]) }).baseline, none);
});

test("the CLI's per-kind baseline wins, and unregistered calls are listed only with a readable registry", () => {
    const commands = [
        { command: "save", tests: ["t"], registered: true },
        { command: "plugin:x|y", tests: ["t"], registered: false },
        { command: "delete", tests: [], registered: true },
    ];
    const baseline = { commands: true, events: false };
    const listed = coverageSummary({ run: null, coverage: coverage([], { baseline, commands, surface: [{ path: "src-tauri/src", commands: 2 }] }) });
    assert.deepEqual(listed.baseline, baseline);
    assert.deepEqual(listed.unexercisedCommands, ["delete"]);
    assert.deepEqual(listed.unregisteredCommands, ["plugin:x|y"]);
    const unreadable = coverageSummary({ run: null, coverage: coverage([], { commands, surface: [{ path: "x", commands: 0, error: "nope" }] }) });
    assert.deepEqual(unreadable.unregisteredCommands, []);
});

test("the run's apps are the service names its suites launched, once each", () => {
    const run = { suites: [{ app: { serviceName: "cutready" } }, { app: null }, { app: { serviceName: "cutready" } }, { app: { serviceName: "" } }, {}] };
    assert.deepEqual(runApps(run), ["cutready"]);
    assert.deepEqual(runApps({}), []);
});
