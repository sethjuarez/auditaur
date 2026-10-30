// Loads what one trace entry recorded: a suite's hooks from run.json, or a test attempt's report
// and telemetry from its attempt folder.

import { join } from "node:path";
import { RUNS_DIR, attemptDir } from "../../lib/paths.mjs";
import { readJson } from "../../lib/files.mjs";
import { errorText } from "../../lib/runs.mjs";

export async function loadAttempt(workspace, run, entry, requestedAttempt) {
    const runDir = join(workspace, RUNS_DIR, run.runId);
    if (entry.kind === "hooks") {
        const suite = run.suites.find((candidate) => `hooks:${candidate.id}` === entry.key);
        return { key: entry.key, kind: "hooks", title: entry.title, attempt: null, status: entry.result, error: errorText(suite?.error), steps: suite?.hooks ?? [], attachments: [], telemetry: null };
    }
    const known = entry.attempts.map((attempt) => attempt.attempt);
    const attempt = known.includes(requestedAttempt) ? requestedAttempt : known.at(-1) ?? 0;
    const report = await readJson(join(runDir, attemptDir(entry.key, attempt), "attempt.json")).catch(() => null);
    const detail = {
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
    return detail;
}