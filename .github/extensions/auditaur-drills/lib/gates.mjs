import { readFile, rename, unlink, writeFile } from "node:fs/promises";
import { basename, dirname, join, resolve } from "node:path";
import { RUNS_DIR } from "./paths.mjs";
import { exists } from "./files.mjs";
import { pickRun } from "./runs.mjs";
import { readEvents } from "./events.mjs";
import { DrillError } from "./errors.mjs";

export const GATE_REQUEST_ID = /^[A-Za-z0-9._-]{1,128}$/;

// Pending gates whose request file is still published, with what the panel needs to answer them.
export async function pendingGates(run, events) {
    if (run.status !== "running") return [];
    const titles = new Map((run.suites ?? []).flatMap((suite) => (suite.tests ?? []).map((test) => [test.id, test.title])));
    const suites = new Map((run.suites ?? []).map((suite) => [suite.id, suite.suite]));
    const gates = [];
    for (const waiting of events.gates) {
        const request = waiting.gate?.request ?? null;
        if (request && !(await exists(request.requestPath))) continue;
        gates.push({
            requestId: request?.requestId ?? null,
            test: waiting.testId ? titles.get(waiting.testId) ?? waiting.testId : `${suites.get(waiting.suiteId) ?? waiting.suiteId} (hook)`,
            attempt: waiting.attempt,
            since: waiting.since,
            name: waiting.gate?.name,
            instructions: waiting.gate?.instructions,
            selector: waiting.gate?.selector ?? null,
            expectText: waiting.gate?.expectText ?? null,
            timeoutMs: waiting.gate?.timeoutMs ?? null,
            manualContinue: waiting.gate?.manualContinue ?? false,
            choices: waiting.gate?.choices ?? [],
            inputs: waiting.gate?.inputs ?? [],
            clipboard: waiting.gate?.clipboard ?? null,
        });
    }
    return gates;
}

// Answers a gate the shown run is waiting on, with the same response file the gate canvas and
// `respond_human_gate` write. Only gates announced in the run's own event log can be answered.
export async function respondGate(state, body) {
    const { run } = await pickRun(state);
    if (!run || run.status !== "running") {
        throw new DrillError("no_active_run", "the shown run is not waiting on a gate");
    }
    const requestId = String(body?.requestId ?? "");
    if (!GATE_REQUEST_ID.test(requestId)) {
        throw new DrillError("invalid_gate", "requestId is required");
    }
    const events = await readEvents(join(state.workspace, RUNS_DIR, run.runId, "events.jsonl"));
    const waiting = events.gates.find((candidate) => candidate.gate?.request?.requestId === requestId);
    const request = waiting?.gate?.request;
    if (!request || !(await exists(request.requestPath))) {
        throw new DrillError("gate_not_pending", `gate \`${requestId}\` is not waiting for an answer`);
    }
    const requestPath = resolve(request.requestPath);
    const responsePath = resolve(request.responsePath);
    const queue = dirname(dirname(requestPath));
    if (
        basename(requestPath) !== `${requestId}.json` ||
        basename(dirname(requestPath)) !== "requests" ||
        basename(queue) !== "human-gates" ||
        responsePath !== join(queue, "responses", `${requestId}.json`)
    ) {
        throw new DrillError("invalid_gate", "the gate's request is outside a human-gates queue");
    }
    // The published request, not just the event log, must name this gate.
    const published = await readFile(requestPath, "utf8")
        .then((text) => JSON.parse(text))
        .catch(() => null);
    if (published?.requestId !== requestId || published?.nonce !== request.nonce) {
        throw new DrillError("gate_not_pending", `gate \`${requestId}\` is not waiting for an answer`);
    }
    const gate = waiting.gate;
    const choices = gate.choices ?? [];
    let choiceId = body?.choiceId ?? null;
    if (choices.length) {
        if (!choices.some((choice) => choice.id === choiceId)) {
            throw new DrillError("invalid_choice", `choose one of: ${choices.map((choice) => choice.id).join(", ")}`);
        }
    } else if (choiceId != null) {
        throw new DrillError("invalid_choice", "this gate has no choices; omit choiceId to continue");
    }
    const given = Array.isArray(body?.inputs) ? body.inputs : [];
    const inputs = (gate.inputs ?? []).map((input) => ({
        id: input.id,
        value: String(given.find((candidate) => candidate?.id === input.id)?.value ?? ""),
    }));
    const chosen = choices.find((choice) => choice.id === choiceId);
    // The runner checks required inputs for every answer except retry.
    if (chosen?.outcome !== "retry") {
        const missing = (gate.inputs ?? []).filter((input, index) => input.required && !inputs[index].value.trim());
        if (missing.length) {
            throw new DrillError("missing_input", `fill in: ${missing.map((input) => input.label ?? input.id).join(", ")}`);
        }
    }
    const response = {
        schemaVersion: 1,
        requestId,
        runId: request.runId,
        gateId: request.gateId,
        attempt: request.attempt,
        nonce: request.nonce,
        responder: "human-confirmed-via-drill-run-canvas",
        choiceId,
        inputs,
    };
    const temp = `${responsePath}.${process.pid}.${Date.now()}.tmp`;
    await writeFile(temp, JSON.stringify(response, null, 2), "utf8");
    await rename(temp, responsePath);
    // The gate may have timed out or been cancelled while we wrote: its cleanup already ran, so the
    // request is gone but our answer is not. Drop it (it can hold sensitive inputs). If both are
    // gone, the runner consumed the answer.
    if (!(await exists(requestPath)) && (await unlink(responsePath).then(() => true, () => false))) {
        throw new DrillError("gate_not_pending", `gate \`${requestId}\` stopped waiting before the answer arrived`);
    }
    return { requestId, choiceId, answered: true };
}
