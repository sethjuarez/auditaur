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
// Reads that arrive while it runs share that run instead of starting another.
async function coverageFor(state, run, fresh) {
    const key = JSON.stringify([run.runId, run.seq, run.status, state.dbs.map(stamp)]);
    if (!fresh && state.cache?.key === key) return state.cache;
    if (state.coverageLoad?.key !== key || (fresh && !state.coverageLoad.fresh)) {
        const args = ["drill", "coverage", run.runId, ...state.dbs.flatMap((db) => ["--db", db])];
        const load = {
            key,
            fresh,
            promise: cliJson(state, args).then(({ value, error }) => {
                const result = { key, value: value ?? null, error: value ? null : error };
                if (state.coverageLoad === load) {
                    state.cache = result;
                    state.coverageLoad = null;
                }
                return result;
            }),
        };
        state.coverageLoad = load;
        load.promise.catch(() => {
            if (state.coverageLoad === load) state.coverageLoad = null;
        });
    }
    return state.coverageLoad.promise;
}

// The apps the run's suites launched, by service name, so the store picker can suggest their stores.
export const runApps = (run) => [...new Set((run.suites ?? []).map((suite) => suite.app?.serviceName).filter((name) => typeof name === "string" && name))];

export async function coverageView(state, { fresh = false } = {}) {
    const { run, runs } = await pickRun(state);
    const view = {
        pinned: state.runId,
        runs: runs.map((candidate) => ({ runId: candidate.runId, status: candidate.status })),
        dbs: state.dbs,
        run: run ? { runId: run.runId, status: run.status, apps: runApps(run) } : null,
        coverage: null,
        error: null,
    };
    if (!run) return view;
    const { value, error } = await coverageFor(state, run, fresh);
    return { ...view, coverage: value, error };
}

const unexercised = (items, key) => items.filter((item) => !item.tests.length).map((item) => item[key]);

const storeBaseline = (coverage) => (coverage.sources ?? []).some((source) => !source.error);
// Per kind: commands also have a baseline in the app's registered commands (CLI 0.4.21+).
export const baselineOf = (coverage) => coverage.baseline ?? { commands: storeBaseline(coverage), events: storeBaseline(coverage) };

export function coverageSummary(view) {
    const coverage = view.coverage;
    if (!coverage) return { run: view.run, error: view.error, dbs: view.dbs };
    return {
        run: view.run,
        sources: coverage.sources,
        surface: coverage.surface ?? [],
        // Without a baseline for a kind, its counts are out of only what the run touched.
        baseline: baselineOf(coverage),
        totals: coverage.totals,
        unexercisedCommands: unexercised(coverage.commands, "command"),
        unexercisedEvents: unexercised(coverage.events, "event"),
        unregisteredCommands: (coverage.surface ?? []).some((surface) => !surface.error)
            ? coverage.commands.filter((item) => !item.registered).map((item) => item.command)
            : [],
        failingCommands: coverage.commands
            .filter((item) => item.failed)
            .map((item) => ({ command: item.command, failed: item.failed, calls: item.calls })),
        testsWithoutTelemetry: coverage.tests
            .filter((test) => test.telemetry !== "recorded")
            .map((test) => ({ title: test.title, telemetry: test.telemetry, error: test.error })),
    };
}
