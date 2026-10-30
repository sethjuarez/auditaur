import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { LIB } from "./lib.mjs";

const { snapshotsView, snapshotsSummary } = await import(new URL("../features/snapshots/view.mjs", LIB));

async function workspaceWith(steps) {
    const workspace = await mkdtemp(join(tmpdir(), "snapshots-view-"));
    const runDir = join(workspace, ".auditaur", "drill-runs", "20261030-000000-000");
    await mkdir(join(runDir, "tests", "t", "attempt-0"), { recursive: true });
    await writeFile(join(runDir, "run.json"), JSON.stringify({
        runId: "20261030-000000-000",
        status: "passed",
        suites: [{ id: "s", suite: "smoke", hooks: [], tests: [{ id: "t", title: "smoke › home", attempts: [{ attempt: 0 }] }] }],
    }));
    await writeFile(join(runDir, "tests", "t", "attempt-0", "attempt.json"), JSON.stringify({ steps }));
    return workspace;
}

test("named screenshot steps are listed, and matched snapshot steps are not results", async (t) => {
    const workspace = await workspaceWith([
        { action: "click", status: "passed", title: "click #go", attachments: [] },
        { action: "screenshot", status: "failed", title: "screenshot broken", attachments: [{ name: "failure", contentType: "image/png", path: "x.png" }] },
        { action: "expect", title: "expect main to match ARIA snapshot", attachments: [] },
        { action: "screenshot", status: "passed", title: "screenshot home", attachments: [{ name: "home", contentType: "image/png", path: "tests/t/attempt-0/step-3-home.png" }] },
    ]);
    t.after(() => rm(workspace, { recursive: true, force: true }));
    const view = await snapshotsView({ workspace, runId: null });
    assert.deepEqual(view.items, []);
    assert.deepEqual(view.screenshots.map(({ step, stepIndex, file }) => ({ step, stepIndex, path: file.path })), [
        { step: "screenshot home", stepIndex: 3, path: "tests/t/attempt-0/step-3-home.png" },
    ]);
    assert.deepEqual(snapshotsSummary(view).screenshots, [
        { suite: "smoke", test: "smoke › home", step: "screenshot home", attachment: "tests/t/attempt-0/step-3-home.png" },
    ]);
});
