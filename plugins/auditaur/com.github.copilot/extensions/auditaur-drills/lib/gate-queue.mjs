// A human gate's answer queue. A waiting drill publishes `<session>/human-gates/requests/<id>.json`
// and polls `<session>/human-gates/responses/<id>.json` for the answer. Every answer the canvas
// writes goes through `publishAnswer`, which only ever writes that one response file, and only for
// a request that is still published and names the same gate.

import { link, readFile, realpath, unlink, writeFile } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import { basename, dirname, isAbsolute, join, resolve } from "node:path";
import { DrillError } from "./errors.mjs";
import { exists } from "./files.mjs";

export const GATE_REQUEST_ID = /^[A-Za-z0-9._-]{1,128}$/;

// `{ requestId, requestPath, responsePath }` for a request file, or a DrillError when the path is
// not one inside a human-gates queue.
export function queuePaths(requestPath) {
    if (typeof requestPath !== "string" || !isAbsolute(requestPath)) {
        throw new DrillError("invalid_gate", "requestPath is the absolute path list_pending_human_gates returns");
    }
    const path = resolve(requestPath);
    const name = basename(path);
    const requestId = name.endsWith(".json") ? name.slice(0, -".json".length) : "";
    const requests = dirname(path);
    const queue = dirname(requests);
    if (!GATE_REQUEST_ID.test(requestId) || basename(requests) !== "requests" || basename(queue) !== "human-gates") {
        throw new DrillError("invalid_gate", "requestPath is not a request in a human-gates queue");
    }
    return { requestId, requestPath: path, responsePath: join(queue, "responses", `${requestId}.json`) };
}

// The physical response path for a queue, after following junctions and symlinks: `requests/` and
// `responses/` must really be siblings under one `human-gates/`, so the answer cannot land elsewhere.
async function physicalResponsePath({ requestId, requestPath }) {
    const real = (path) => realpath(path).catch(() => null);
    const queue = dirname(dirname(requestPath));
    const [physicalQueue, requests, responses] = await Promise.all([
        real(queue),
        real(dirname(requestPath)),
        real(join(queue, "responses")),
    ]);
    if (
        !physicalQueue ||
        basename(physicalQueue) !== "human-gates" ||
        requests !== join(physicalQueue, "requests") ||
        responses !== join(physicalQueue, "responses")
    ) {
        throw new DrillError("invalid_gate", "requestPath is not a request in a human-gates queue");
    }
    return join(responses, `${requestId}.json`);
}

// The published request, or null once it is gone (answered, timed out, or the drill stopped).
export async function readRequest(requestPath) {
    const request = await readFile(requestPath, "utf8")
        .then((text) => JSON.parse(text))
        .catch(() => null);
    return request && typeof request === "object" && !Array.isArray(request) ? request : null;
}

// Expiry is in Unix nanoseconds; millisecond precision is plenty.
export const isExpired = (request, now = Date.now()) =>
    Number.isFinite(Number(request?.expiresAtUnixNanos)) && Number(request.expiresAtUnixNanos) / 1e6 <= now;

// A request's Unix-nanosecond timestamp as ISO, or null.
export const isoFromNanos = (nanos) =>
    nanos != null && Number.isFinite(Number(nanos)) ? new Date(Number(nanos) / 1e6).toISOString() : null;

// The answer `body` gives `gate`, checked as the runner will: a listed choice (none when the gate
// has no choices) and every required input, except for a retry.
export function checkAnswer(gate, body) {
    const choices = Array.isArray(gate?.choices) ? gate.choices : [];
    const fields = Array.isArray(gate?.inputs) ? gate.inputs : [];
    const choiceId = body?.choiceId ?? null;
    if (choices.length) {
        if (!choices.some((choice) => choice.id === choiceId)) {
            throw new DrillError("invalid_choice", `choose one of: ${choices.map((choice) => choice.id).join(", ")}`);
        }
    } else if (choiceId != null) {
        throw new DrillError("invalid_choice", "this gate has no choices; omit choiceId to continue");
    }
    const given = Array.isArray(body?.inputs) ? body.inputs : [];
    const inputs = fields.map((input) => ({
        id: input.id,
        value: String(given.find((candidate) => candidate?.id === input.id)?.value ?? ""),
    }));
    if (choices.find((choice) => choice.id === choiceId)?.outcome !== "retry") {
        const missing = fields.filter((input, index) => input.required && !inputs[index].value.trim());
        if (missing.length) {
            throw new DrillError("missing_input", `fill in: ${missing.map((input) => input.label ?? input.id).join(", ")}`);
        }
    }
    return { choiceId, inputs };
}

const notPending = (requestId, why = "is not waiting for an answer") => new DrillError("gate_not_pending", `gate \`${requestId}\` ${why}`);

// Answers the request at `requestPath`. `expected` pins what the caller showed (e.g. the nonce from
// the run's event log), and `gate` the choices and inputs it offered; both default to the published
// request's own.
export async function publishAnswer({ requestPath, expected = {}, gate, body, responder }) {
    // Every answer says who gave it; the drill report keeps it as the audit trail.
    if (typeof responder !== "string" || !responder.trim()) throw new TypeError("publishAnswer needs a responder");
    const paths = queuePaths(requestPath);
    const { requestId, requestPath: path } = paths;
    if (expected.requestId != null && expected.requestId !== requestId) throw notPending(expected.requestId);
    const published = await readRequest(path);
    if (
        published?.requestId !== requestId ||
        typeof published.nonce !== "string" ||
        (expected.nonce != null && published.nonce !== expected.nonce)
    ) {
        throw notPending(requestId);
    }
    if (isExpired(published)) throw notPending(requestId, "timed out");
    const answer = checkAnswer(gate ?? published, body);
    const response = {
        schemaVersion: 1,
        requestId,
        runId: published.runId,
        gateId: published.gateId,
        attempt: published.attempt,
        nonce: published.nonce,
        responder,
        ...answer,
    };
    const responsePath = await physicalResponsePath(paths);
    const temp = `${responsePath}.${randomUUID()}.tmp`;
    await writeFile(temp, JSON.stringify(response, null, 2), "utf8");
    // Link, not rename: publishing is atomic and never replaces an answer already given, so the
    // first answer wins even when the human and the agent answer at once.
    const linked = await link(temp, responsePath).then(
        () => true,
        (error) => (error.code === "EEXIST" ? false : Promise.reject(error)),
    ).finally(() => unlink(temp).catch(() => {}));
    if (!linked) throw notPending(requestId, "already has an answer");
    // The gate may have timed out or been cancelled while we wrote: its cleanup already ran, so the
    // request is gone but our answer is not. Drop it (it can hold sensitive inputs). If both are
    // gone, the runner consumed the answer.
    if (!(await exists(path)) && (await unlink(responsePath).then(() => true, () => false))) {
        throw notPending(requestId, "stopped waiting before the answer arrived");
    }
    return { requestId, choiceId: answer.choiceId, answered: true };
}
