import { test } from "node:test";
import assert from "node:assert/strict";
import { lib } from "./lib.mjs";

const { cliVersion, codeFiles, versionSkew } = await lib("build-info.mjs");

test("the stale check covers provider code but not the page bundle, which is served fresh", async () => {
    const names = await codeFiles();
    assert.ok(names.includes("extension.mjs"));
    assert.ok(names.includes("lib/server.mjs"));
    assert.deepEqual(names.filter((name) => name.startsWith("ui/")), []);
});

test("a CLI that differs from the plugin is a skew; unknown versions are not", () => {
    assert.deepEqual(versionSkew("0.5.0", "0.4.8"), { plugin: "0.5.0", cli: "0.4.8" });
    assert.equal(versionSkew("0.5.0", "0.5.0"), null);
    assert.equal(versionSkew(null, "0.4.8"), null);
    assert.equal(versionSkew("0.5.0", null), null);
});

test("the CLI version is cached briefly, then checked again", async () => {
    let runs = 0;
    const run = async () => ({ version: `0.0.${++runs}`, error: null });
    const cli = `fake-cli-${Math.random()}`;
    assert.equal((await cliVersion(cli, run, 0)).version, "0.0.1");
    assert.equal((await cliVersion(cli, run, 30_000)).version, "0.0.1");
    assert.equal((await cliVersion(cli, run, 61_000)).version, "0.0.2");
});
