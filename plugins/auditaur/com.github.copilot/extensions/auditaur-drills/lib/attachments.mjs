// A run's attachments, reachable only when the run's own report lists them.

import { readFile, realpath, stat } from "node:fs/promises";
import { isAbsolute, join, relative, resolve } from "node:path";
import { RUNS_DIR, RUN_ID, attemptDir } from "./paths.mjs";
import { DrillError } from "./errors.mjs";
import { readJson } from "./files.mjs";
import { readRun } from "./runs.mjs";
import { RAW } from "./server.mjs";

const FILE_LIMIT = 32 * 1024 * 1024;
const ATTEMPT_PATH = /^tests\/([A-Za-z0-9_-]{1,128})\/attempt-(\d{1,4})\//;

async function listedAttachments(state, runId, runDir, path) {
    const inAttempt = ATTEMPT_PATH.exec(path);
    if (inAttempt) {
        const report = await readJson(join(runDir, attemptDir(inAttempt[1], Number(inAttempt[2])), "attempt.json")).catch(() => null);
        return [...(report?.attachments ?? []), ...(report?.steps ?? []).flatMap((step) => step.attachments ?? [])];
    }
    if (path.startsWith("suites/")) {
        const run = await readRun(state, runId);
        return (run?.suites ?? []).flatMap((suite) => (suite.hooks ?? []).flatMap((step) => step.attachments ?? []));
    }
    return [];
}

// The attachment's report entry and its real file, after checking it is listed and stays in the run.
export async function resolveAttachment(state, runId, path) {
    if (!RUN_ID.test(String(runId))) throw new DrillError("invalid_run", "not a drill run id");
    const runDir = resolve(state.workspace, RUNS_DIR, runId);
    const attachment = (await listedAttachments(state, runId, runDir, String(path))).find((candidate) => candidate.path === path);
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
    return { attachment, full };
}

export async function readAttachment(state, runId, path) {
    const { attachment, full } = await resolveAttachment(state, runId, path);
    return { attachment, bytes: await readFile(full) };
}

// `GET /file?runId=&path=`: images as themselves, text as plain text, anything else as bytes.
export async function serveAttachment(state, url) {
    const { attachment, bytes } = await readAttachment(state, url.searchParams.get("runId") ?? "", url.searchParams.get("path") ?? "");
    const type = String(attachment.contentType ?? "");
    const contentType = /^image\/(png|jpeg|gif|webp)$/.test(type)
        ? type
        : /^(application\/json|text\/)/.test(type)
          ? "text/plain; charset=utf-8"
          : "application/octet-stream";
    return { [RAW]: { contentType, body: bytes } };
}