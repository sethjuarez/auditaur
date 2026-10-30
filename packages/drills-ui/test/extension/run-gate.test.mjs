import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { Readable } from "node:stream";
import { RUN, lib, tempDir } from "./lib.mjs";

const { loadFeatures } = await lib("features.mjs");
const { features } = await loadFeatures();
const run = features.find((feature) => feature.page === "run");

const nanos = (ms) => String(BigInt(Math.round(ms)) * 1_000_000n);

// A workspace whose running run waits on one published gate.
async function waitingRun() {
    const workspace = await tempDir("drills-run-gate-");
    const queue = join(workspace, "human-gates");
    await mkdir(join(queue, "requests"), { recursive: true });
    await mkdir(join(queue, "responses"), { recursive: true });
    const requestId = "gate-1";
    const requestPath = join(queue, "requests", `${requestId}.json`);
    const responsePath = join(queue, "responses", `${requestId}.json`);
    const gate = {
        name: "Check it",
        instructions: "Look.",
        choices: [{ id: "ok", label: "OK", outcome: "continue" }],
        inputs: [],
    };
    await writeFile(requestPath, JSON.stringify({
        schemaVersion: 1, requestId, runId: RUN, gateId: "g", attempt: 0, nonce: "n-1",
        createdAtUnixNanos: nanos(Date.now()), expiresAtUnixNanos: nanos(Date.now() + 60_000),
        session: { sessionId: "s", pid: 1 }, prompt: { name: gate.name, instructions: gate.instructions },
        choices: gate.choices, inputs: gate.inputs,
    }));
    const runDir = join(workspace, ".auditaur", "drill-runs", RUN);
    await mkdir(runDir, { recursive: true });
    await writeFile(join(runDir, "run.json"), JSON.stringify({ runId: RUN, status: "running", pid: process.pid, suites: [] }));
    await writeFile(join(runDir, "events.jsonl"), JSON.stringify({
        type: "gateWaiting", suiteId: "s", testId: "t", attempt: 0, step: 0, ts: new Date().toISOString(),
        gate: { ...gate, request: { requestId, requestPath, responsePath, nonce: "n-1" } },
    }) + "\n");
    return { state: { workspace, runId: null }, responsePath };
}

const responderOf = async (path) => JSON.parse(await readFile(path, "utf8")).responder;

test("a waiting gate counts down to the runner's own deadline", async () => {
    const { state } = await waitingRun();
    const { pendingGates } = await lib("gates.mjs");
    const { readEvents } = await lib("events.mjs");
    const events = await readEvents(join(state.workspace, ".auditaur", "drill-runs", RUN, "events.jsonl"));
    const [gate] = await pendingGates({ status: "running", suites: [] }, events);
    const request = JSON.parse(await readFile(join(state.workspace, "human-gates", "requests", "gate-1.json"), "utf8"));
    assert.equal(gate.expiresAt, new Date(Number(request.expiresAtUnixNanos) / 1e6).toISOString());
});

test("the run page's form records a human answer", async () => {
    const { state, responsePath } = await waitingRun();
    const req = Readable.from([Buffer.from(JSON.stringify({ requestId: "gate-1", choiceId: "ok" }))]);
    await run.routes(state)["POST /gate"](null, req);
    assert.equal(await responderOf(responsePath), "human-confirmed-via-drill-run-canvas");
});

test("respond_gate records an agent answer", async () => {
    const { state, responsePath } = await waitingRun();
    const action = run.actions.find((candidate) => candidate.name === "respond_gate");
    await action.handler(state, { requestId: "gate-1", choiceId: "ok" });
    assert.equal(await responderOf(responsePath), "agent-via-drill-run-canvas");
});

test("an answer without a responder is refused", async () => {
    const { state, responsePath } = await waitingRun();
    const { respondGate } = await lib("gates.mjs");
    await assert.rejects(respondGate(state, { requestId: "gate-1", choiceId: "ok" }), /needs a responder/);
    await assert.rejects(readFile(responsePath), { code: "ENOENT" });
});
