import { test } from "node:test";
import assert from "node:assert/strict";
import { RUN, lib } from "./lib.mjs";

const { parseRoute, formatRoute, canonicalRoute } = await lib("route.mjs");

test("parses pages and typed params", () => {
    assert.deepEqual(parseRoute("/tests"), { page: "tests", params: {} });
    assert.deepEqual(parseRoute(`/trace?run=${RUN}&test=hooks:suite-1&attempt=2`), {
        page: "trace",
        params: { run: RUN, test: "hooks:suite-1", attempt: 2 },
    });
});

test("canonical routes order params and drop trailing slashes", () => {
    assert.equal(canonicalRoute(`/trace/?attempt=0&test=a&run=${RUN}`), `/trace?run=${RUN}&test=a&attempt=0`);
    assert.equal(formatRoute("run", { run: null }), "/run");
});

test("rejects what no page takes", () => {
    for (const bad of ["/nope", "/tests?run=" + RUN, "/run?run=../x", "/trace?attempt=1", "/trace?test=a b", 42, "/" + "x".repeat(700)]) {
        assert.throws(() => parseRoute(bad), (error) => error.code === "invalid_route", String(bad));
    }
});