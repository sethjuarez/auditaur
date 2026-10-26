// The snapshots canvas's view model: every snapshot step of a run that did not match its baseline.

import { join } from "node:path";
import { RUNS_DIR, attemptDir } from "../../lib/paths.mjs";
import { DrillError } from "../../lib/errors.mjs";
import { readJson } from "../../lib/files.mjs";
import { HISTORY_RUNS, errorText, readAllRuns } from "../../lib/runs.mjs";
import { readAttachment } from "../../lib/attachments.mjs";
import { baselineKind, baselinePath, readBaseline, writeBaseline } from "../../lib/baselines.mjs";

const FILES = ["expected", "actual", "diff"];

// A step is a snapshot result when its error names the baseline it was checked against.
function snapshotStep(step, index, owner) {
    const baseline = step.error?.expected?.snapshot;
    if (typeof baseline !== "string") return null;
    const files = {};
    for (const attachment of step.attachments ?? []) {
        if (FILES.includes(attachment.name)) files[attachment.name] = { path: attachment.path, contentType: attachment.contentType };
    }
    return {
        id: `${owner.key}/${owner.attempt ?? "hooks"}/${index}`,
        suite: owner.suite,
        test: owner.title,
        stepIndex: index,
        step: step.title,
        kind: baselineKind(baseline),
        baseline,
        // The runner marks output that never settled; it must not become a baseline.
        stable: step.error.expected.stable !== false,
        message: errorText(step.error),
        files,
    };
}

async function collect(state, run) {
    const runDir = join(state.workspace, RUNS_DIR, run.runId);
    const items = [];
    for (const suite of run.suites ?? []) {
        const hooks = { key: `hooks:${suite.id}`, suite: suite.suite, title: "suite hooks", attempt: null };
        (suite.hooks ?? []).forEach((step, index) => items.push(snapshotStep(step, index, hooks)));
        for (const test of suite.tests ?? []) {
            // Only the last attempt counts: a retry checks (and may rewrite) the same baselines.
            const attempt = (test.attempts ?? []).map((each) => each.attempt).filter(Number.isSafeInteger).at(-1);
            if (attempt == null) continue;
            const report = await readJson(join(runDir, attemptDir(test.id, attempt), "attempt.json")).catch(() => null);
            const owner = { key: test.id, suite: suite.suite, title: test.title, attempt };
            (report?.steps ?? []).forEach((step, index) => items.push(snapshotStep(step, index, owner)));
        }
    }
    return items.filter(Boolean);
}

async function actualBytes(state, runId, item) {
    if (!item.files.actual) return null;
    return (await readAttachment(state, runId, item.files.actual.path).catch(() => null))?.bytes ?? null;
}

// Whether the baseline on disk still differs from what the run saw.
async function withStatus(state, runId, item) {
    const actual = await actualBytes(state, runId, item);
    if (!actual) return { ...item, status: "noActual" };
    if (!item.stable) return { ...item, status: "unstable" };
    try {
        const baseline = await readBaseline(baselinePath(state.workspace, item.baseline));
        return { ...item, status: baseline?.equals(actual) ? "accepted" : baseline ? "changed" : "missing" };
    } catch (error) {
        return { ...item, status: "unreadable", message: `${item.message}\n\ncould not read the baseline: ${error.message}` };
    }
}

async function shownRun(state) {
    const all = await readAllRuns(state);
    const runs = all.slice(0, HISTORY_RUNS);
    const run = state.runId ? all.find((candidate) => candidate.runId === state.runId) ?? null : all[0] ?? null;
    if (run && !runs.some((candidate) => candidate.runId === run.runId)) runs.push(run);
    return { run, runs };
}

export async function snapshotsView(state) {
    const { run, runs } = await shownRun(state);
    const view = {
        workspace: state.workspace,
        runs: runs.map((candidate) => ({ runId: candidate.runId, status: candidate.status })),
        pinned: state.runId,
        run: null,
        items: [],
    };
    if (!run) return view;
    view.run = { runId: run.runId, status: run.status, startedAt: run.startedAt };
    view.items = await Promise.all((await collect(state, run)).map((item) => withStatus(state, run.runId, item)));
    return view;
}

export function snapshotsSummary(view) {
    return {
        runId: view.run?.runId ?? null,
        status: view.run?.status ?? null,
        snapshots: view.items.map(({ id, suite, test, step, kind, baseline, status, message, files }) => ({
            id,
            suite,
            test,
            step,
            kind,
            baseline,
            status,
            message,
            attachments: Object.fromEntries(Object.entries(files).map(([name, file]) => [name, file.path])),
        })),
    };
}

// Copies each chosen step's actual output over its baseline. Ids and paths come only from the run's
// own report, and every id is checked before anything is written.
export async function acceptSnapshots(state, runId, ids) {
    const run = (await readAllRuns(state)).find((candidate) => candidate.runId === runId);
    if (!run) throw new DrillError("invalid_run", `run ${runId} was not found`);
    // A running run may still compare against these baselines.
    if (run.status === "running") throw new DrillError("run_active", `run ${runId} is still running`);
    const items = await collect(state, run);
    const writes = new Map();
    for (const id of new Set(ids)) {
        const item = items.find((candidate) => candidate.id === id);
        if (!item) throw new DrillError("not_snapshot", `run ${runId} has no snapshot result \`${id}\``);
        if (!item.stable) throw new DrillError("unstable", `\`${id}\` never stabilized; fix the flake and re-run`);
        const bytes = await actualBytes(state, runId, item);
        if (!bytes) throw new DrillError("no_actual", `\`${id}\` has no actual output to accept`);
        const full = baselinePath(state.workspace, item.baseline);
        const same = writes.get(full);
        if (same && !same.bytes.equals(bytes)) {
            throw new DrillError("conflict", `\`${same.ids[0]}\` and \`${id}\` saw different output for \`${item.baseline}\`; accept one`);
        }
        if (same) same.ids.push(id);
        else writes.set(full, { ids: [id], baseline: item.baseline, full, bytes });
    }
    for (const write of writes.values()) await writeBaseline(state.workspace, write.full, write.bytes);
    return {
        runId,
        accepted: [...writes.values()].flatMap(({ ids: written, baseline }) => written.map((id) => ({ id, baseline }))),
    };
}
