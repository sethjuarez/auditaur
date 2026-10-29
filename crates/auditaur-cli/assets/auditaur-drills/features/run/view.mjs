// The run canvas's view model: one run's suites, tests, attempts, and steps, plus waiting gates.

import { join } from "node:path";
import { RUNS_DIR } from "../../lib/paths.mjs";
import { errorText, pickRun, resultOf } from "../../lib/runs.mjs";
import { readEvents } from "../../lib/events.mjs";
import { pendingGates } from "../../lib/gates.mjs";
export async function liveView(state) {
    const { run, runs } = await pickRun(state);
    const view = {
        workspace: state.workspace,
        pinned: state.runId ?? null,
        runs: runs.map((candidate) => ({ runId: candidate.runId, status: candidate.status })),
        run: null,
        gates: [],
    };
    if (!run) return view;
    const events = await readEvents(join(state.workspace, RUNS_DIR, run.runId, "events.jsonl"));
    const live = run.status === "running";
    const stepsFor = (suiteId, testId, attempt) =>
        (events.steps.get(`${suiteId}|${testId ?? ""}|${attempt ?? ""}`) ?? []).map((step) =>
            // A step left running by a dead or finished run never finished.
            !live && step.status === "running" ? { ...step, status: "interrupted" } : step,
        );
    view.run = {
        runId: run.runId,
        status: run.status,
        startedAt: run.startedAt,
        durationMs: run.durationMs,
        totals: run.totals,
        suites: (run.suites ?? []).map((suite) => ({
            id: suite.id,
            suite: suite.suite,
            path: suite.path,
            status: suite.status,
            durationMs: suite.durationMs,
            error: errorText(suite.error),
            hooks: stepsFor(suite.id, null, null),
            tests: (suite.tests ?? []).map((test) => ({
                id: test.id,
                name: test.name,
                title: test.title,
                tags: test.tags ?? [],
                result: resultOf(test),
                durationMs: test.durationMs,
                attempts: (test.attempts ?? []).map((attempt) => ({
                    attempt: attempt.attempt,
                    status: !live && attempt.status === "running" ? "interrupted" : attempt.status,
                    durationMs: attempt.durationMs,
                    error: errorText(attempt.error),
                    steps: stepsFor(suite.id, test.id, attempt.attempt),
                })),
            })),
        })),
    };
    view.gates = await pendingGates(run, events);
    return view;
}

export function liveSummary(view) {
    const run = view.run;
    if (!run) return { workspace: view.workspace, run: null, gates: [] };
    const tests = run.suites.flatMap((suite) => suite.tests);
    return {
        runId: run.runId,
        status: run.status,
        totals: run.totals,
        durationMs: run.durationMs,
        running: tests.filter((test) => test.result === "running").map((test) => test.title),
        failed: tests
            .filter((test) => test.result === "failed")
            .map((test) => ({ title: test.title, error: test.attempts.at(-1)?.error ?? null })),
        gates: view.gates.map(({ requestId, test, name, instructions, choices, inputs }) => ({
            requestId,
            test,
            name,
            instructions,
            choices,
            inputs: inputs.map(({ id, label, kind, required }) => ({ id, label, kind, required })),
        })),
    };
}
