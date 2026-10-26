import { test } from "node:test";
import assert from "node:assert/strict";
import { RUN, lib } from "./lib.mjs";

const { AppPanel } = await lib("app-panel.mjs");
const { loadFeatures } = await lib("features.mjs");
const { features } = await loadFeatures();

const make = (options = {}) => new AppPanel({ features, input: {}, ...options });

test("a saved route wins over the replayed open input", () => {
    const panel = make({ input: { route: `/run?run=${RUN}` }, saved: { route: "/coverage" } });
    assert.equal(panel.route.path, "/coverage");
});

test("input routes a new panel; invalid ones fall back to tests", () => {
    assert.equal(make({ input: { route: `/run?run=${RUN}` } }).route.path, `/run?run=${RUN}`);
    assert.equal(make({ input: { route: "/bogus" } }).route.path, "/tests");
    assert.equal(make({ saved: { route: "/bogus" }, input: { route: "/run" } }).route.path, "/run");
});

test("navigate commits a new revision, sets the selection, and notifies", () => {
    const panel = make();
    const seen = [];
    panel.subscribe((version) => seen.push(version));
    const route = panel.navigate(`/trace?attempt=1&test=t1&run=${RUN}`);
    assert.equal(route.rev, 1);
    assert.equal(route.path, `/trace?run=${RUN}&test=t1&attempt=1`);
    assert.deepEqual([panel.shared.runId, panel.shared.testId, panel.shared.attempt], [RUN, "t1", 1]);
    assert.equal(route.recent.trace, route.path);
    assert.equal(seen.length, 1);
    assert.throws(() => panel.navigate("/nope"), (error) => error.code === "invalid_route");
    assert.equal(panel.route.rev, 1);
});

test("each page has its own view over the shared state", () => {
    const panel = make();
    panel.page("tests").view.set({ filter: "smoke" });
    assert.equal(panel.page("tests").state.view.get().filter, "smoke");
    assert.notEqual(panel.page("coverage").state.view, panel.page("tests").view);
    panel.page("coverage").state.dbs = ["a.sqlite"];
    assert.deepEqual(panel.shared.dbs, ["a.sqlite"]);
});

test("saved views and stores restore; invalid fields keep defaults", () => {
    const panel = make({ saved: { views: { tests: { filter: "x", bogus: 1 }, coverage: { tab: "nope" } }, dbs: ["s.sqlite"] } });
    assert.equal(panel.page("tests").view.get().filter, "x");
    assert.equal(panel.page("coverage").view.get().tab, "commands");
    assert.ok(panel.shared.trusted.has("s.sqlite"));
});

test("changes save a snapshot, and dispose stops saving", async () => {
    const saves = [];
    const panel = make({ save: async (data) => saves.push(data) });
    panel.navigate("/coverage");
    await new Promise((resolve) => setTimeout(resolve, 400));
    assert.equal(saves.at(-1).route, "/coverage");
    panel.navigate("/run");
    panel.dispose();
    await new Promise((resolve) => setTimeout(resolve, 400));
    assert.equal(saves.length, 1);
});

test("a page off screen acts on its own last route, not the one on screen", () => {
    const panel = make();
    panel.navigate(`/trace?run=${RUN}&test=t1`);
    panel.navigate("/coverage");
    const trace = panel.page("trace").state;
    assert.deepEqual([trace.runId, trace.testId], [RUN, "t1"]);
    assert.equal(panel.page("coverage").state.runId, null);
    assert.equal(panel.page("snapshots").state.runId, null);
    assert.equal(panel.routeOf("trace"), `/trace?run=${RUN}&test=t1`);
    assert.equal(panel.routeOf("snapshots"), "/snapshots");
    assert.throws(() => (trace.runId = "other"), TypeError);
    assert.equal(trace.runId, RUN);
});

test("saves run one at a time and the last one written is the newest", async () => {
    const written = [];
    let slow = true;
    const panel = make({
        save: async (data) => {
            if (slow) {
                slow = false;
                await new Promise((resolve) => setTimeout(resolve, 400));
            }
            written.push(data.route);
        },
    });
    panel.navigate("/coverage");
    await new Promise((resolve) => setTimeout(resolve, 300));
    panel.navigate("/run");
    await new Promise((resolve) => setTimeout(resolve, 700));
    assert.deepEqual(written, ["/coverage", "/run"]);
});

test("a page that failed to load is reported, not crashed on", () => {
    const panel = new AppPanel({ features: features.filter((feature) => feature.page !== "trace"), input: {} });
    assert.throws(() => panel.page("trace"), (error) => error.code === "page_unavailable");
    assert.equal(panel.page("tests").feature.page, "tests");
});