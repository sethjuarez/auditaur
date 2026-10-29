import { test } from "node:test";
import assert from "node:assert/strict";
import { RUN, lib } from "./lib.mjs";

const { canvasActions, runAction } = await lib("actions.mjs");
const { AppPanel } = await lib("app-panel.mjs");
const { loadFeatures } = await lib("features.mjs");
const { features } = await loadFeatures();

const byName = (actions) => Object.fromEntries(actions.map((action) => [action.name, action]));

test("the action set is pinned", () => {
    assert.deepEqual(canvasActions(features, async () => {}).map((action) => action.name).sort(), [
        "accept_snapshots", "answer_gate", "cancel", "get_coverage", "get_gate", "get_snapshots", "get_status",
        "get_summary", "get_trace", "get_view", "health", "list_app_stores", "navigate", "refresh", "respond_gate",
        "run", "screenshot", "set_sources", "set_view", "show_gate",
    ]);
});

test("duplicate action names are refused", () => {
    const clash = { page: "extra", routes: () => ({}), actions: [{ name: "navigate", handler: () => {} }] };
    assert.throws(() => canvasActions([...features, clash], async () => {}), /duplicate/);
});

test("set_view changes another page's view without navigating", async () => {
    const actions = byName(canvasActions(features, async () => {}));
    const panel = new AppPanel({ features, input: {} });
    let notified = 0;
    panel.subscribe(() => notified++);
    await runAction(actions.set_view, panel, { page: "snapshots", view: { mode: "diff" } });
    assert.equal(panel.page("snapshots").view.get().mode, "diff");
    assert.equal(panel.route.page, "tests");
    assert.ok(notified >= 1);
});

test("navigate formats the route, or opens a new panel", async () => {
    const opened = [];
    const actions = byName(canvasActions(features, async (from, path) => (opened.push(path), { route: path })));
    const panel = new AppPanel({ features, input: {} });
    await runAction(actions.navigate, panel, { page: "run", runId: RUN, newPanel: true });
    assert.deepEqual(opened, [`/run?run=${RUN}`]);
    assert.equal(panel.route.path, "/tests");
    await runAction(actions.navigate, panel, { page: "snapshots", runId: RUN });
    assert.equal(panel.route.path, `/snapshots?run=${RUN}`);
});

test("read-only actions do not notify pages", async () => {
    const actions = byName(canvasActions(features, async () => {}));
    const panel = new AppPanel({ features, input: {} });
    let notified = 0;
    panel.subscribe(() => notified++);
    await runAction(actions.health, panel, {});
    assert.equal(notified, 0);
});