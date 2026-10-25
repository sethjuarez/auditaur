import { readFile, realpath, stat } from "node:fs/promises";
import { isAbsolute, join, relative, resolve } from "node:path";
import { RUNS_DIR, RUN_ID, attemptDir } from "../../lib/paths.mjs";
import { DrillError } from "../../lib/errors.mjs";
import { readJson } from "../../lib/files.mjs";
import { readRun } from "../../lib/runs.mjs";
import { RAW } from "../../lib/server.mjs";

const FILE_LIMIT = 32 * 1024 * 1024;
// Serves one attachment of one run, only if the run's own report lists it.
export async function traceFile(state, url) {
    const runId = url.searchParams.get("runId") ?? "";
    const path = url.searchParams.get("path") ?? "";
    if (!RUN_ID.test(runId)) throw new DrillError("invalid_run", "not a drill run id");
    const runDir = resolve(state.workspace, RUNS_DIR, runId);
    let listed = [];
    const inAttempt = /^tests\/([A-Za-z0-9_-]{1,128})\/attempt-(\d{1,4})\//.exec(path);
    if (inAttempt) {
        const report = await readJson(join(runDir, attemptDir(inAttempt[1], Number(inAttempt[2])), "attempt.json")).catch(() => null);
        listed = [...(report?.attachments ?? []), ...(report?.steps ?? []).flatMap((step) => step.attachments ?? [])];
    } else if (path.startsWith("suites/")) {
        const run = await readRun(state, runId);
        listed = (run?.suites ?? []).flatMap((suite) => (suite.hooks ?? []).flatMap((step) => step.attachments ?? []));
    }
    const attachment = listed.find((candidate) => candidate.path === path);
    if (!attachment) throw new DrillError("not_attachment", "not an attachment of this run");
    // Resolve links before checking containment, so a symlink or junction cannot escape the run.
    const [realRun, full] = await Promise.all([realpath(runDir), realpath(resolve(runDir, path))]).catch(() => []);
    if (!full) throw new DrillError("not_found", "the attachment file is missing");
    const inside = relative(realRun, full);
    if (!inside || inside.startsWith("..") || isAbsolute(inside)) {
        throw new DrillError("not_attachment", "not an attachment of this run");
    }
    const info = await stat(full).catch(() => null);
    if (!info?.isFile()) throw new DrillError("not_found", "the attachment file is missing");
    if (info.size > FILE_LIMIT) throw new DrillError("too_large", "the attachment is too large to show");
    const type = String(attachment.contentType ?? "");
    const contentType = /^image\/(png|jpeg|gif|webp)$/.test(type)
        ? type
        : /^(application\/json|text\/)/.test(type)
          ? "text/plain; charset=utf-8"
          : "application/octet-stream";
    return { [RAW]: { contentType, body: await readFile(full) } };
}
