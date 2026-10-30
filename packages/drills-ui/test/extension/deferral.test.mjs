import { test } from "node:test";
import assert from "node:assert/strict";
import { lib } from "./lib.mjs";

const { awaitCommittedCanvas, missingCanvasMessage } = await lib("deferral.mjs");
const noSleep = async () => {};

test("an installed copy that stepped aside sees the committed canvas register", async () => {
    let calls = 0;
    const list = async () => ({ canvases: ++calls < 3 ? [] : [{ canvasId: "auditaur-drills", extensionId: "project:auditaur-drills" }] });
    assert.equal(await awaitCommittedCanvas({ list, canvasId: "auditaur-drills", sleep: noSleep }), "registered");
    assert.equal(calls, 3);
});

test("it reports the canvas missing when nothing registers within the grace period", async () => {
    const list = async () => ({ canvases: [{ canvasId: "browser", extensionId: "builtin" }] });
    assert.equal(await awaitCommittedCanvas({ list, canvasId: "auditaur-drills", graceMs: 0, sleep: noSleep }), "missing");
});

test("a host that can't list canvases is unknown, not missing", async () => {
    assert.equal(await awaitCommittedCanvas({ list: undefined, canvasId: "auditaur-drills" }), "unknown");
    const failing = async () => {
        throw new Error("method not found");
    };
    assert.equal(await awaitCommittedCanvas({ list: failing, canvasId: "auditaur-drills" }), "unknown");
    assert.equal(await awaitCommittedCanvas({ list: async () => ({}), canvasId: "auditaur-drills" }), "unknown");
});

test("the warning names the committed copy and the fixes", () => {
    const message = missingCanvasMessage("/repo/.github/extensions/auditaur-drills/extension.mjs", 20_000);
    assert.match(message, /\/repo\/\.github\/extensions\/auditaur-drills\/extension\.mjs/);
    assert.match(message, /20 s after this session started/);
    assert.match(message, /auditaur init extension --force/);
    assert.match(message, /delete it to use the installed copy/);
});
