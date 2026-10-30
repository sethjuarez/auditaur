// What the trace page can show for a run: each suite's hooks and each test, with its attempts.

import { resultOf } from "../../lib/runs.mjs";

const FAILED_RESULTS = ["failed", "flaky", "interrupted"];

export function traceEntries(run) {
    const entries = [];
    for (const suite of run.suites ?? []) {
        if ((suite.hooks ?? []).length) {
            const failed = suite.hooks.some((step) => ["failed", "timedOut"].includes(step.status));
            entries.push({ key: `hooks:${suite.id}`, kind: "hooks", suite: suite.suite, title: `${suite.suite} › suite hooks`, result: failed ? "failed" : "passed", attempts: [] });
        }
        for (const test of suite.tests ?? []) {
            entries.push({
                key: test.id,
                kind: "test",
                suite: suite.suite,
                title: test.title,
                result: resultOf(test),
                attempts: (test.attempts ?? []).map((attempt) => ({ attempt: attempt.attempt, status: attempt.status })),
            });
        }
    }
    return entries;
}

// The routed entry, else the first failed test, else the first test that ran, else the first entry.
export function pickEntry(entries, testId) {
    return (
        entries.find((candidate) => candidate.key === testId) ??
        entries.find((candidate) => candidate.kind === "test" && FAILED_RESULTS.includes(candidate.result)) ??
        entries.find((candidate) => candidate.kind === "test" && candidate.attempts.length) ??
        entries[0]
    );
}