import { test } from "node:test";
import assert from "node:assert/strict";
import { lib } from "./lib.mjs";

const { codeFiles } = await lib("build-info.mjs");

test("the stale check covers provider code but not the page bundle, which is served fresh", async () => {
    const names = await codeFiles();
    assert.ok(names.includes("extension.mjs"));
    assert.ok(names.includes("lib/server.mjs"));
    assert.deepEqual(names.filter((name) => name.startsWith("ui/")), []);
});
