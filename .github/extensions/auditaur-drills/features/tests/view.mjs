// The explorer's view model: every suite and test with its latest result and recent history, and
// whether the view's filter shows it.

import { loadSuites } from "../../lib/cli.mjs";
import { readRuns, resultOf } from "../../lib/runs.mjs";

function matches(test, filter) {
    const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
    const haystack = `${test.title} ${(test.tags ?? []).map((tag) => `@${tag}`).join(" ")}`.toLowerCase();
    return words.every((word) => haystack.includes(word));
}

export function buildView(state, runs) {
    const { filter } = state.view.get();
    const byRun = runs.map((run) => {
        const tests = new Map();
        for (const suite of run.suites ?? []) {
            for (const test of suite.tests ?? []) {
                tests.set(test.id, {
                    result: resultOf(test),
                    durationMs: test.durationMs,
                    error: test.attempts?.at(-1)?.error?.message ?? null,
                });
            }
        }
        return { runId: run.runId, tests };
    });
    const suites = (state.list?.suites ?? []).map((suite) => ({
        suite: suite.suite,
        path: suite.path,
        description: suite.description,
        tags: suite.tags ?? [],
        tests: (suite.tests ?? []).map((test) => {
            const history = byRun
                .filter((run) => run.tests.has(test.id))
                .map((run) => ({ runId: run.runId, ...run.tests.get(test.id) }));
            const last = history.find((entry) => entry.result !== "notRun") ?? null;
            return {
                id: test.id,
                name: test.name,
                title: test.title,
                tags: test.tags ?? [],
                mode: test.mode,
                modeReason: test.modeReason,
                slow: test.slow,
                steps: test.steps ?? [],
                last,
                // Oldest first, so the newest result sits at the right edge.
                history: history.slice().reverse(),
                flaky: history.some((entry) => entry.result === "flaky"),
                shown: matches(test, filter),
            };
        }),
    }));
    const active = runs.find((run) => run.status === "running") ?? null;
    let progress = null;
    if (active) {
        const tests = (active.suites ?? []).flatMap((suite) => suite.tests ?? []);
        progress = {
            runId: active.runId,
            done: tests.filter((test) => !["pending", "running"].includes(test.status)).length,
            total: tests.length,
            current: tests.find((test) => test.status === "running")?.title ?? null,
        };
    }
    return {
        workspace: state.workspace,
        cli: state.cli,
        listError: state.listError,
        invalid: state.list?.invalid ?? [],
        suites,
        runs: runs.map((run) => ({
            runId: run.runId,
            status: run.status,
            totals: run.totals,
            durationMs: run.durationMs,
            startedAt: run.startedAt,
        })),
        active: progress,
        launch: state.launch,
    };
}

export async function snapshot(state, force = false) {
    await loadSuites(state, force);
    return buildView(state, await readRuns(state));
}

// The tests the filter leaves on screen, as the agent would name them.
export function visibleTests(view) {
    return view.suites.flatMap((suite) => suite.tests).filter((test) => test.shown).map((test) => ({ id: test.id, title: test.title, result: test.last?.result ?? "notRun" }));
}

export function summary(view) {
    const tests = view.suites.flatMap((suite) => suite.tests);
    return {
        workspace: view.workspace,
        listError: view.listError,
        invalidFiles: view.invalid.length,
        suites: view.suites.length,
        tests: tests.length,
        lastRun: view.runs.find((run) => run.status !== "running") ?? null,
        active: view.active,
        failing: tests.filter((test) => test.last?.result === "failed").map((test) => test.title),
        flaky: tests.filter((test) => test.flaky).map((test) => test.title),
        neverRun: tests.filter((test) => !test.last).map((test) => test.title),
    };
}
