// The coverage canvas's view model: `drill coverage --json` for the shown run.

import { statSync } from "node:fs";
import { cliJson } from "../../lib/cli.mjs";
import { pickRun } from "../../lib/runs.mjs";

// A store's size and mtime, plus its write-ahead log's, so new app telemetry invalidates the cache.
function stamp(db) {
    return [db, `${db}-wal`].map((path) => {
        try {
            const { size, mtimeMs } = statSync(path);
            return `${size}:${mtimeMs}`;
        } catch {
            return "-";
        }
    });
}

// `drill coverage` reads every attempt folder, so it is re-run only when the run or a store changes, or on refresh.
async function coverageFor(state, run, fresh) {
    const key = JSON.stringify([run.runId, run.seq, run.status, state.dbs.map(stamp)]);
    if (fresh || state.cache?.key !== key) {
        const args = ["drill", "coverage", run.runId, ...state.dbs.flatMap((db) => ["--db", db])];
        const { value, error } = await cliJson(state, args);
        state.cache = { key, value: value ?? null, error: value ? null : error };
    }
    return state.cache;
}

export async function coverageView(state, { fresh = false } = {}) {
    const { run, runs } = await pickRun(state);
    const view = {
        pinned: state.runId,
        runs: runs.map((candidate) => ({ runId: candidate.runId, status: candidate.status })),
        dbs: state.dbs,
        run: run ? { runId: run.runId, status: run.status } : null,
        coverage: null,
        error: null,
    };
    if (!run) return view;
    const { value, error } = await coverageFor(state, run, fresh);
    return { ...view, coverage: value, error };
}

const unexercised = (items, key) => items.filter((item) => !item.tests.length).map((item) => item[key]);

export function coverageSummary(view) {
    const coverage = view.coverage;
    if (!coverage) return { run: view.run, error: view.error, dbs: view.dbs };
    return {
        run: view.run,
        sources: coverage.sources,
        totals: coverage.totals,
        unexercisedCommands: unexercised(coverage.commands, "command"),
        unexercisedEvents: unexercised(coverage.events, "event"),
        failingCommands: coverage.commands
            .filter((item) => item.failed)
            .map((item) => ({ command: item.command, failed: item.failed, calls: item.calls })),
        testsWithoutTelemetry: coverage.tests
            .filter((test) => test.telemetry !== "recorded")
            .map((test) => ({ title: test.title, telemetry: test.telemetry, error: test.error })),
    };
}
