import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdir, readdir, readFile, rm, symlink, unlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { LIB, lib, tempDir } from "./lib.mjs";

const { queuePaths, publishAnswer, checkAnswer } = await lib("gate-queue.mjs");
const { gateView } = await import(new URL("../features/gate/view.mjs", LIB));
const { canvasActions, runAction } = await lib("actions.mjs");
const { AppPanel } = await lib("app-panel.mjs");
const { inheritedInput } = await lib("config.mjs");
const { loadFeatures } = await lib("features.mjs");
const { features } = await loadFeatures();

const nanos = (ms) => String(BigInt(Math.round(ms)) * 1_000_000n);

// A human-gates queue with one published request; returns its paths.
async function publish(request = {}) {
    const queue = join(await tempDir("drills-gate-"), "human-gates");
    await mkdir(join(queue, "requests"), { recursive: true });
    await mkdir(join(queue, "responses"), { recursive: true });
    const requestId = request.requestId ?? "gate-1";
    const requestPath = join(queue, "requests", `${requestId}.json`);
    await writeFile(requestPath, JSON.stringify({
        schemaVersion: 1,
        requestId,
        runId: "run-1",
        gateId: "g",
        attempt: 0,
        nonce: "n-1",
        createdAtUnixNanos: nanos(Date.now()),
        expiresAtUnixNanos: nanos(Date.now() + 60_000),
        session: { sessionId: "s", pid: 1 },
        prompt: { name: "Check the dialog", instructions: "Look at it." },
        choices: [{ id: "pass", label: "Pass", outcome: "pass" }, { id: "retry", label: "Retry", outcome: "retry" }],
        inputs: [{ id: "note", label: "Note", required: true }],
        ...request,
    }));
    return { requestPath, responsePath: join(queue, "responses", `${requestId}.json`) };
}

const answer = { choiceId: "pass", inputs: [{ id: "note", value: "ok" }] };

test("queuePaths accepts only requests inside a human-gates queue", () => {
    const base = join(tmpdir(), "s", "human-gates");
    const paths = queuePaths(join(base, "requests", "a.json"));
    assert.equal(paths.requestId, "a");
    assert.equal(paths.responsePath, join(base, "responses", "a.json"));
    for (const bad of ["relative/human-gates/requests/a.json", join(tmpdir(), "elsewhere", "requests", "a.json"), join(base, "responses", "a.json"), join(base, "requests", "a.txt"), join(base, "requests", "a b.json"), 7]) {
        assert.throws(() => queuePaths(bad), { code: "invalid_gate" }, String(bad));
    }
});

test("checkAnswer enforces choices and required inputs, except for a retry", () => {
    const gate = { choices: [{ id: "pass" }, { id: "retry", outcome: "retry" }], inputs: [{ id: "note", required: true }] };
    assert.throws(() => checkAnswer(gate, { choiceId: "nope" }), { code: "invalid_choice" });
    assert.throws(() => checkAnswer(gate, { choiceId: "pass" }), { code: "missing_input" });
    assert.deepEqual(checkAnswer(gate, { choiceId: "retry" }), { choiceId: "retry", inputs: [{ id: "note", value: "" }] });
    assert.throws(() => checkAnswer({}, { choiceId: "pass" }), { code: "invalid_choice" });
    assert.deepEqual(checkAnswer({}, {}), { choiceId: null, inputs: [] });
});

test("publishAnswer writes the response the runner expects", async () => {
    const { requestPath, responsePath } = await publish();
    const result = await publishAnswer({ requestPath, body: answer, responder: "test" });
    assert.deepEqual(result, { requestId: "gate-1", choiceId: "pass", answered: true });
    const response = JSON.parse(await readFile(responsePath, "utf8"));
    assert.deepEqual(response, {
        schemaVersion: 1, requestId: "gate-1", runId: "run-1", gateId: "g", attempt: 0, nonce: "n-1",
        responder: "test", choiceId: "pass", inputs: [{ id: "note", value: "ok" }],
    });
});

test("publishAnswer refuses a gate that is not the one shown or no longer waiting", async () => {
    const { requestPath, responsePath } = await publish();
    await assert.rejects(publishAnswer({ requestPath, expected: { nonce: "other" }, body: answer, responder: "test" }), { code: "gate_not_pending" });
    await assert.rejects(publishAnswer({ requestPath, expected: { requestId: "gate-2" }, body: answer, responder: "test" }), { code: "gate_not_pending" });
    await unlink(requestPath);
    await assert.rejects(publishAnswer({ requestPath, body: answer, responder: "test" }), { code: "gate_not_pending" });

    const expired = await publish({ expiresAtUnixNanos: nanos(Date.now() - 1000) });
    await assert.rejects(publishAnswer({ requestPath: expired.requestPath, body: answer, responder: "test" }), /timed out/);

    const renamed = await publish({ requestId: "gate-1" });
    await writeFile(renamed.requestPath, JSON.stringify({ requestId: "other", nonce: "n" }));
    await assert.rejects(publishAnswer({ requestPath: renamed.requestPath, body: answer, responder: "test" }), { code: "gate_not_pending" });
    await assert.rejects(readFile(responsePath), { code: "ENOENT" });
});

test("the first answer wins; a second never replaces it", async () => {
    const { requestPath, responsePath } = await publish();
    const retry = { choiceId: "retry", inputs: [] };
    const results = await Promise.allSettled([
        publishAnswer({ requestPath, body: answer, responder: "human" }),
        publishAnswer({ requestPath, body: retry, responder: "agent" }),
    ]);
    assert.equal(results.filter((result) => result.status === "fulfilled").length, 1);
    assert.match(results.find((result) => result.status === "rejected").reason.message, /already has an answer/);
    const winner = results[0].status === "fulfilled" ? "human" : "agent";
    assert.equal(JSON.parse(await readFile(responsePath, "utf8")).responder, winner);
    await assert.rejects(publishAnswer({ requestPath, body: answer, responder: "late" }), /already has an answer/);
    assert.deepEqual((await readdir(join(responsePath, ".."))).filter((name) => name.endsWith(".tmp")), []);
});

test("publishAnswer refuses a responses directory that links elsewhere", async () => {
    const { requestPath, responsePath } = await publish();
    const elsewhere = await tempDir("drills-elsewhere-");
    await rm(join(responsePath, ".."), { recursive: true });
    await symlink(elsewhere, join(responsePath, ".."), "junction");
    await assert.rejects(publishAnswer({ requestPath, body: answer, responder: "test" }), { code: "invalid_gate" });
    assert.deepEqual(await readdir(elsewhere), []);
});

test("gateView reports each status", async () => {
    assert.equal((await gateView({ gate: null })).status, "none");
    const { requestPath, responsePath } = await publish();
    const pending = await gateView({ gate: requestPath });
    assert.equal(pending.status, "pending");
    assert.equal(pending.gate.name, "Check the dialog");
    assert.equal(pending.gate.choices.length, 2);
    await writeFile(responsePath, "{}");
    assert.equal((await gateView({ gate: requestPath })).status, "answered");
    await unlink(requestPath);
    assert.equal((await gateView({ gate: requestPath })).status, "closed");
    const expired = await publish({ expiresAtUnixNanos: nanos(Date.now() - 1000) });
    assert.equal((await gateView({ gate: expired.requestPath })).status, "expired");
});

test("a panel opened with a gate starts on /gate; new panels don't inherit it", async () => {
    const { requestPath } = await publish();
    const panel = new AppPanel({ features, input: { gate: { requestPath, requestId: "gate-1" } } });
    assert.equal(panel.route.path, "/gate");
    assert.equal(panel.snapshot().gate, requestPath);
    assert.equal(new AppPanel({ features, input: { gate: { requestPath }, route: "/tests" } }).route.path, "/tests");
    assert.deepEqual(inheritedInput({ cwd: "x", route: "/gate", gate: { requestPath } }), { cwd: "x" });
    assert.throws(() => new AppPanel({ features, input: { gate: { requestPath: "nope" } } }), { code: "invalid_gate" });
});

test("a saved gate wins over the input's on rehydrate", async () => {
    const first = await publish({ requestId: "first" });
    const second = await publish({ requestId: "second" });
    const panel = new AppPanel({ features, input: { gate: { requestPath: first.requestPath } }, saved: { route: "/gate", gate: second.requestPath } });
    assert.equal(panel.shared.gate, second.requestPath);
});

test("show_gate navigates, and answer_gate answers the shown gate", async () => {
    const actions = Object.fromEntries(canvasActions(features, async () => {}).map((action) => [action.name, action]));
    const panel = new AppPanel({ features, input: {} });
    await assert.rejects(runAction(actions.answer_gate, panel, answer), { code: "no_gate" });
    const { requestPath, responsePath } = await publish();
    const view = await runAction(actions.show_gate, panel, { requestPath });
    assert.equal(panel.route.path, "/gate");
    assert.equal(view.status, "pending");
    await runAction(actions.answer_gate, panel, answer);
    assert.equal(JSON.parse(await readFile(responsePath, "utf8")).responder, "agent-via-drills-canvas");
});
