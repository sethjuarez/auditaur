import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import { RUNS_DIR, RUN_ID } from "./paths.mjs";
import { processAlive } from "./processes.mjs";

export const HISTORY_RUNS = 10;

export async function readRun(state, id) {
    try {
        const report = JSON.parse(await readFile(join(state.workspace, RUNS_DIR, id, "run.json"), "utf8"));
        // Same rule as `drill show`: a running run whose owner died was interrupted.
        if (report.status === "running" && !processAlive(report.pid)) {
            report.status = "interrupted";
            for (const suite of report.suites ?? []) {
                for (const test of suite.tests ?? []) {
                    if (test.status === "running") test.status = "interrupted";
                }
            }
        }
        return report;
    } catch {
        // Missing, being written, or pruned; the next poll picks it up.
        return null;
    }
}

// Every run, newest first. Pruning keeps this short; `HISTORY_RUNS` only limits what is shown.
export async function readAllRuns(state) {
    let ids;
    try {
        ids = (await readdir(join(state.workspace, RUNS_DIR), { withFileTypes: true }))
            .filter((entry) => entry.isDirectory() && RUN_ID.test(entry.name))
            .map((entry) => entry.name)
            .sort()
            .reverse();
    } catch {
        return [];
    }
    const runs = [];
    for (const id of ids) {
        const report = await readRun(state, id);
        if (report) runs.push(report);
    }
    return runs;
}

export async function readRuns(state) {
    return (await readAllRuns(state)).slice(0, HISTORY_RUNS);
}

// One test's result in one run, folded to what the explorer shows.
export function resultOf(test) {
    if (!test || test.status === "pending") return "notRun";
    if (test.status === "running") return "running";
    if (test.outcome === "flaky") return "flaky";
    if (test.status === "skipped") return "skipped";
    if (test.status === "interrupted") return "interrupted";
    if (test.outcome === "expected") return "passed";
    return "failed";
}

export const errorText = (error) => (error == null ? null : typeof error === "string" ? error : error.message ?? JSON.stringify(error));

export async function pickRun(state) {
    const all = await readAllRuns(state);
    const runs = all.slice(0, HISTORY_RUNS);
    let run;
    if (state.runId) {
        run = all.find((candidate) => candidate.runId === state.runId) ?? (await readRun(state, state.runId));
    } else {
        // Follow the workspace: the run in progress (however old), else the newest one.
        run = all.find((candidate) => candidate.status === "running") ?? all[0] ?? null;
    }
    if (run && !runs.some((candidate) => candidate.runId === run.runId)) runs.push(run);
    return { run, runs };
}
