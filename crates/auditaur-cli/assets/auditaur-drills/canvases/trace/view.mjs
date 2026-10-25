// The trace canvas's view model: one attempt's steps and telemetry on a shared time axis.

import { join } from "node:path";
import { RUNS_DIR, attemptDir } from "../../lib/paths.mjs";
import { readJson } from "../../lib/files.mjs";
import { HISTORY_RUNS, errorText, readAllRuns, resultOf } from "../../lib/runs.mjs";
import { isoMs, num, traceEvents } from "../../lib/telemetry.mjs";
function traceEntries(run) {
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

export async function traceView(state) {
    const all = await readAllRuns(state);
    const runs = all.slice(0, HISTORY_RUNS);
    const run = state.runId ? all.find((candidate) => candidate.runId === state.runId) ?? null : all[0] ?? null;
    if (run && !runs.some((candidate) => candidate.runId === run.runId)) runs.push(run);
    const view = {
        workspace: state.workspace,
        runs: runs.map((candidate) => ({ runId: candidate.runId, status: candidate.status })),
        pinned: state.runId,
        run: null,
        entries: [],
        selected: null,
    };
    if (!run) return view;
    view.run = { runId: run.runId, status: run.status, startedAt: run.startedAt, totals: run.totals };
    view.entries = traceEntries(run);
    const entry =
        view.entries.find((candidate) => candidate.key === state.testId) ??
        view.entries.find((candidate) => candidate.kind === "test" && ["failed", "flaky", "interrupted"].includes(candidate.result)) ??
        view.entries.find((candidate) => candidate.kind === "test" && candidate.attempts.length) ??
        view.entries[0];
    if (!entry) return view;
    view.selected = await traceDetail(state, run, entry);
    return view;
}

async function traceDetail(state, run, entry) {
    const runDir = join(state.workspace, RUNS_DIR, run.runId);
    let detail;
    if (entry.kind === "hooks") {
        const suite = run.suites.find((candidate) => `hooks:${candidate.id}` === entry.key);
        detail = { key: entry.key, kind: "hooks", title: entry.title, attempt: null, status: entry.result, error: errorText(suite?.error), steps: suite?.hooks ?? [], attachments: [], telemetry: null };
    } else {
        const known = entry.attempts.map((attempt) => attempt.attempt);
        const attempt = known.includes(state.attempt) ? state.attempt : known.at(-1) ?? 0;
        const report = await readJson(join(runDir, attemptDir(entry.key, attempt), "attempt.json")).catch(() => null);
        detail = {
            key: entry.key,
            kind: "test",
            title: entry.title,
            attempt,
            status: report?.status ?? entry.attempts.find((each) => each.attempt === attempt)?.status ?? "pending",
            startedAt: report?.startedAt ?? null,
            durationMs: report?.durationMs ?? null,
            error: errorText(report?.error),
            steps: report?.steps ?? [],
            attachments: report?.attachments ?? [],
            missing: !report,
            telemetry: null,
        };
        const telemetry = detail.attachments.find((attachment) => attachment.name === "telemetry");
        if (telemetry) detail.telemetry = await readJson(join(runDir, telemetry.path)).catch(() => null);
    }
    const starts = detail.steps.map((step) => isoMs(step.startedAt));
    const origin = isoMs(detail.startedAt) ?? starts.find((start) => start != null) ?? null;
    const events = traceEvents(detail.telemetry);
    const offset = (at) => (origin == null || at == null ? null : Math.round((at - origin) * 10) / 10);
    // Each event belongs to the latest step that started before it: effects often land just after
    // the step that caused them returns.
    const stepOf = (at) => {
        let found = null;
        starts.forEach((start, index) => {
            if (start != null && start <= at) found = index;
        });
        return found;
    };
    return {
        key: detail.key,
        kind: detail.kind,
        title: detail.title,
        attempt: num(detail.attempt),
        status: detail.status,
        durationMs: num(detail.durationMs),
        error: detail.error,
        missing: detail.missing ?? false,
        filterNotes: detail.telemetry?.filterNotes ?? null,
        steps: detail.steps.map((step, index) => ({
            index,
            phase: step.phase,
            title: step.title,
            action: step.action,
            status: step.status,
            soft: step.soft ?? false,
            offsetMs: offset(starts[index]),
            durationMs: num(step.durationMs),
            error: errorText(step.error),
            attachments: (step.attachments ?? []).map(({ name, contentType, path }) => ({ name, contentType, path })),
        })),
        attachments: detail.attachments
            .filter((attachment) => attachment.name !== "telemetry")
            .map(({ name, contentType, path }) => ({ name, contentType, path })),
        events: events.map((event) => ({ ...event, offsetMs: offset(event.at), step: stepOf(event.at) })),
    };
}

export function traceSummary(view) {
    const selected = view.selected;
    return {
        runId: view.run?.runId ?? null,
        status: view.run?.status ?? null,
        tests: view.entries.map(({ key, title, result, attempts }) => ({ testId: key, title, result, attempts: attempts.length })),
        selected: selected && {
            testId: selected.key,
            title: selected.title,
            attempt: selected.attempt,
            status: selected.status,
            error: selected.error,
            steps: selected.steps.map((step) => ({
                index: step.index,
                title: step.title,
                status: step.status,
                offsetMs: step.offsetMs,
                durationMs: step.durationMs,
                error: step.error,
                attachments: step.attachments.map((attachment) => attachment.path),
                telemetry: selected.events
                    .filter((event) => event.step === step.index)
                    .map(({ kind, name, status, offsetMs, detail }) => ({ kind, name, status, offsetMs, detail: detail.slice(0, 200) })),
            })),
            beforeFirstStep: selected.events.filter((event) => event.step == null).length,
        },
    };
}
