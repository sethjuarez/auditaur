import { test } from "node:test";
import assert from "node:assert/strict";
import { LIB } from "./lib.mjs";

const { coverageSummary, hasBaseline, runApps } = await import(new URL("../features/coverage/view.mjs", LIB));

const coverage = (sources) => ({ sources, totals: {}, commands: [], events: [], tests: [] });

test("coverage has a baseline only when a store could be read", () => {
    assert.equal(hasBaseline(coverage([])), false);
    assert.equal(hasBaseline(coverage([{ db: "a", error: "no telemetry store" }])), false);
    assert.equal(hasBaseline(coverage([{ db: "a", error: "x" }, { db: "b" }])), true);
    assert.equal(coverageSummary({ run: null, coverage: coverage([]) }).baseline, false);
});

test("the run's apps are the service names its suites launched, once each", () => {
    const run = { suites: [{ app: { serviceName: "cutready" } }, { app: null }, { app: { serviceName: "cutready" } }, { app: { serviceName: "" } }, {}] };
    assert.deepEqual(runApps(run), ["cutready"]);
    assert.deepEqual(runApps({}), []);
});
