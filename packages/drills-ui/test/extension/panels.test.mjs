import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { RUN, lib } from "./lib.mjs";

const { Panels, CANVAS_ID } = await lib("panels.mjs");
const { PanelStore } = await lib("panel-store.mjs");
const { loadFeatures } = await lib("features.mjs");
const { features } = await loadFeatures();

// An API URL of the panel at `url`, with its capability token.
const api = (url, path) => {
    const target = new URL(path, url);
    target.searchParams.set("t", new URL(url).searchParams.get("t"));
    return target;
};
const post = (url, path, body) => fetch(api(url, path), { method: "POST", body: JSON.stringify(body) });

async function setup() {
    const workspacePath = await mkdtemp(join(tmpdir(), "drills-panels-"));
    const store = new PanelStore();
    const calls = [];
    const panels = new Panels({ features, store });
    panels.bind({ workspacePath, rpc: { canvas: { open: async (request) => calls.push(request) } } });
    return { panels, store, calls };
}

test("open is idempotent and ignores replayed input", async (t) => {
    const { panels } = await setup();
    t.after(() => panels.close("p"));
    const first = await panels.open("p", { route: "/run" });
    first.panel.navigate("/coverage");
    const again = await panels.open("p", { route: "/run" });
    assert.equal(again, first);
    assert.equal(again.panel.route.path, "/coverage");
    const response = await fetch(api(first.url, "api/route"));
    assert.equal((await response.json()).path, "/coverage");
});

test("a reopened panel resumes its saved route; close forgets it", async () => {
    const { panels, store } = await setup();
    const { panel } = await panels.open("p", {});
    panel.navigate("/snapshots");
    await store.save("p", panel.snapshot());
    await panels.close("p");
    assert.equal(await store.load("p"), null);
    await store.save("q", { route: "/coverage" });
    const reopened = await panels.open("q", { route: "/run" });
    assert.equal(reopened.panel.route.path, "/coverage");
    await panels.close("q");
});

test("new panels get a valid id and inherit everything but the route", async () => {
    const { panels, calls } = await setup();
    const { panel } = await panels.open("p", { cwd: ".", route: "/run", dbs: ["a.sqlite"] });
    const result = await panels.openNew(panel, `/trace?test=t&run=${RUN}`);
    assert.match(result.instanceId, /^auditaur-drills-[0-9a-f-]{36}$/);
    assert.match(result.instanceId, /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/);
    assert.deepEqual(calls[0], {
        canvasId: CANVAS_ID,
        instanceId: result.instanceId,
        input: { cwd: ".", dbs: ["a.sqlite"], route: `/trace?run=${RUN}&test=t` },
    });
    await assert.rejects(panels.openNew(panel, "/nope"), (error) => error.code === "invalid_route");
    await panels.close("p");
});

test("the HTTP API serves the shell, routes, and page state", async () => {
    const { panels } = await setup();
    const { url } = await panels.open("p", {});
    const shell = await fetch(url);
    assert.match(await shell.text(), /<div id="root"><\/div>/);
    const posted = await post(url, "api/route", { path: "/coverage" });
    assert.deepEqual(await posted.json(), { path: "/coverage", page: "coverage", rev: 1, recent: { tests: "/tests", coverage: "/coverage" } });
    assert.equal((await post(url, "api/route", { path: "/x" })).status, 409);
    assert.equal((await (await post(url, "api/tests/view", { filter: "a" })).json()).filter, "a");
    assert.equal((await fetch(api(url, "api/nope"))).status, 404);
    await panels.close("p");
});

test("the API refuses requests without the panel's token, for another host, or too large", async () => {
    const { panels } = await setup();
    const { url } = await panels.open("p", {});
    const token = new URL(url).searchParams.get("t");
    assert.ok(token.length >= 32);
    assert.equal((await fetch(new URL("api/route", url))).status, 403);
    assert.equal((await fetch(new URL(`api/route?t=${token}x`, url))).status, 403);
    assert.equal((await fetch(new URL("api/route", url), { headers: { "X-Drills-Token": token } })).status, 200);
    const { request } = await import("node:http");
    const status = await new Promise((resolve, reject) => {
        const req = request(api(url, "api/route"), { headers: { Host: "evil.example" } }, (res) => resolve(res.statusCode));
        req.on("error", reject);
        req.end();
    });
    assert.equal(status, 421);
    const huge = await fetch(api(url, "api/route"), { method: "POST", body: JSON.stringify({ path: "x".repeat(2 << 20) }) });
    assert.equal(huge.status, 409);
    await panels.close("p");
});

test("close waits for a save in flight, so the panel's file stays removed", async () => {
    const { panels, store } = await setup();
    const { panel } = await panels.open("p", {});
    let release;
    const gate = new Promise((resolve) => (release = resolve));
    const save = store.save.bind(store);
    store.save = async (id, data) => {
        await gate;
        return save(id, data);
    };
    panel.navigate("/coverage");
    await new Promise((resolve) => setTimeout(resolve, 350));
    const closing = panels.close("p");
    release();
    await closing;
    assert.equal(await store.load("p"), null);
});