import { test } from "node:test";
import assert from "node:assert/strict";
import { lib } from "./lib.mjs";

const { loadFeatures } = await lib("features.mjs");

test("all pages load", async () => {
    const { features, failed } = await loadFeatures();
    assert.deepEqual(failed, []);
    assert.deepEqual(features.map((feature) => feature.page), ["tests", "run", "trace", "snapshots", "coverage", "gate"]);
});

test("a broken page is isolated", async () => {
    const { features, failed } = await loadFeatures(["tests", "missing"]);
    assert.deepEqual(features.map((feature) => feature.page), ["tests"]);
    assert.equal(failed[0].page, "missing");
});