import { test } from "node:test";
import assert from "node:assert/strict";
import { lib } from "./lib.mjs";

const { cliVersion, codeFiles, versionSkew } = await lib("build-info.mjs");

test("the stale check covers provider code and version stamps but not the page bundle, which is served fresh", async () => {
    const names = await codeFiles();
    assert.ok(names.includes("extension.mjs"));
    assert.ok(names.includes("lib/server.mjs"));
    assert.ok(names.includes("version.json"), "a new version on disk must show as stale");
    assert.deepEqual(names.filter((name) => name.startsWith("ui/")), []);
});

test("a CLI that differs from the canvas is a skew, naming how the canvas was installed", () => {
    assert.deepEqual(versionSkew({ version: "0.5.0", source: "plugin" }, "0.4.8"), { canvas: "0.5.0", cli: "0.4.8", source: "plugin" });
    assert.deepEqual(versionSkew({ version: "0.4.15", source: "init" }, "0.4.16"), { canvas: "0.4.15", cli: "0.4.16", source: "init" });
    assert.equal(versionSkew({ version: "0.5.0", source: "plugin" }, "0.5.0"), null);
    assert.equal(versionSkew({ version: null, source: null }, "0.4.8"), null);
    assert.equal(versionSkew({ version: "0.5.0", source: "init" }, null), null);
});

test("the CLI version is cached briefly, then checked again", async () => {
    let runs = 0;
    const run = async () => ({ version: `0.0.${++runs}`, error: null });
    const cli = `fake-cli-${Math.random()}`;
    assert.equal((await cliVersion(cli, run, 0)).version, "0.0.1");
    assert.equal((await cliVersion(cli, run, 30_000)).version, "0.0.1");
    assert.equal((await cliVersion(cli, run, 61_000)).version, "0.0.2");
});
