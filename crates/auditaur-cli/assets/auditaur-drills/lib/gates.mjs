import { join, resolve } from "node:path";
import { RUNS_DIR } from "./paths.mjs";
import { exists } from "./files.mjs";
import { pickRun } from "./runs.mjs";
import { readEvents } from "./events.mjs";
import { DrillError } from "./errors.mjs";
import { GATE_REQUEST_ID, publishAnswer, queuePaths } from "./gate-queue.mjs";

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

// Answers a gate the shown run is waiting on. Only gates announced in the run's own event log can
// be answered, with the choices and inputs that log offered.
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
    if (!request) {
        throw new DrillError("gate_not_pending", `gate \`${requestId}\` is not waiting for an answer`);
    }
    if (resolve(String(request.responsePath ?? "")) !== queuePaths(request.requestPath).responsePath) {
        throw new DrillError("invalid_gate", "the gate's request is outside a human-gates queue");
    }
    return publishAnswer({
        requestPath: request.requestPath,
        expected: { requestId, nonce: request.nonce },
        gate: waiting.gate,
        body,
        responder: "human-confirmed-via-drill-run-canvas",
    });
}