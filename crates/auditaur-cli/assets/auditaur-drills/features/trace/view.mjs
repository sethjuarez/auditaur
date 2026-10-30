// The trace page's view model: which run and attempt the route selects, and the step it focuses.

import { HISTORY_RUNS, readAllRuns } from "../../lib/runs.mjs";
import { loadAttempt } from "./attempt.mjs";
import { pickEntry, traceEntries } from "./entries.mjs";
import { alignTimeline } from "./timeline.mjs";

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
    const entry = pickEntry(view.entries, state.testId);
    if (!entry) return view;
    view.selected = alignTimeline(await loadAttempt(state.workspace, run, entry, state.attempt));
    view.selected.step = shownStep(state, run.runId, view.selected);
    return view;
}

// The step the view shows: "auto" is the failing step (or the whole attempt when none failed), "all"
// the whole attempt. Another attempt coming into view (a new latest run, another test) puts the step
// back to auto and tells open pages, so every reader agrees on what is shown.
function shownStep(state, runId, selected) {
    state.view.scope(`${runId}|${selected.key}|${selected.attempt}`, "step");
    const { step } = state.view.get();
    if (step === "auto") return selected.steps.find((each) => ["failed", "timedOut"].includes(each.status))?.index ?? null;
    if (step === "all") return null;
    return step < selected.steps.length ? step : null;
}