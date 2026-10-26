import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, readdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { lib } from "./lib.mjs";

const { PanelStore } = await lib("panel-store.mjs");

async function boundStore() {
    const workspace = await mkdtemp(join(tmpdir(), "drills-store-"));
    const store = new PanelStore();
    store.bind(workspace);
    return { store, dir: join(workspace, "auditaur-drills", "panels") };
}

test("saves, loads, and removes a panel", async () => {
    const { store, dir } = await boundStore();
    await store.save("p-1", { route: "/run" });
    assert.deepEqual(await store.load("p-1"), { route: "/run" });
    assert.deepEqual(await readdir(dir), ["p-1.json"]);
    await store.remove("p-1");
    assert.equal(await store.load("p-1"), null);
});

test("ignores corrupt, outdated, and unsafe files", async () => {
    const { store, dir } = await boundStore();
    await store.save("seed", {});
    await writeFile(join(dir, "corrupt.json"), "{nope");
    await writeFile(join(dir, "old.json"), JSON.stringify({ version: 0, data: { route: "/run" } }));
    assert.equal(await store.load("corrupt"), null);
    assert.equal(await store.load("old"), null);
    assert.equal(await store.load("../seed"), null);
    await store.save("../escape", {});
    assert.ok(!(await readdir(dir)).some((name) => name.includes("escape")));
});

test("without a workspace nothing is kept", async () => {
    const store = new PanelStore();
    await store.save("p", { route: "/run" });
    assert.equal(await store.load("p"), null);
});