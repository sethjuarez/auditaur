// Tests page: browse suites and tests with their latest results; run drills.

import { startRun } from "../../lib/launcher.mjs";
import { readBody } from "../../lib/server.mjs";
import { field } from "../../lib/view-state.mjs";
import { snapshot, summary, visibleTests } from "./view.mjs";

const runSchema = {
    type: "object",
    additionalProperties: false,
    properties: {
        test: { type: "string", description: "Run one test by its `suite > test` title or id." },
        suite: { type: "string", description: "Run one suite by name." },
        grep: { type: "string", description: "Run tests whose `suite > test @tags` match this regex." },
        lastFailed: { type: "boolean", description: "Rerun the tests that failed or did not run last time." },
    },
};

export default {
    page: "tests",
    view: {
        filter: field.text().about("Show only tests whose `suite > test @tags` contain every word."),
        expanded: field.ids().about("Test ids whose steps and last error are expanded."),
    },
    describe: async (state) => ({ shown: visibleTests(await snapshot(state)) }),
    routes: (state) => ({
        "GET /state": (url) => snapshot(state, url.searchParams.has("refresh")),
        "POST /run": async (_url, req) => startRun(state, await readBody(req)),
    }),
    actions: [
        {
            name: "get_summary",
            description: "Suites, tests, the latest run, and which tests are failing, flaky, or have never run.",
            readOnly: true,
            handler: async (state) => summary(await snapshot(state)),
        },
        {
            name: "refresh",
            description: "Reload drill files and run results.",
            handler: async (state) => summary(await snapshot(state, true)),
        },
        {
            name: "run",
            description: "Start `drill test` in the background for everything, one suite, one test, a grep, or the last failures.",
            inputSchema: runSchema,
            handler: (state, input) => startRun(state, input),
        },
    ],
};