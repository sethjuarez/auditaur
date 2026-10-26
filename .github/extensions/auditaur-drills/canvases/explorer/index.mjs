// Explorer canvas: browse suites and tests with their latest results; run or cancel drills.

import { cliFor, workspaceFor } from "../../lib/paths.mjs";
import { cancelRun, startRun } from "../../lib/launcher.mjs";
import { DrillError } from "../../lib/errors.mjs";
import { cwdProperty } from "../../lib/schemas.mjs";
import { readBody } from "../../lib/server.mjs";
import { field } from "../../lib/view-state.mjs";
import { snapshot, summary, visibleTests } from "./view.mjs";
const openSchema = {
    type: "object",
    additionalProperties: false,
    properties: {
        cwd: cwdProperty,
        cli: {
            type: "string",
            description: "Path to the auditaur CLI. Defaults to $AUDITAUR_BIN, then `auditaur` on PATH.",
        },
        run: {
            type: "object",
            additionalProperties: false,
            description: "How the canvas's run buttons launch `drill test`.",
            properties: {
                app: { type: "string", description: "--app name." },
                command: {
                    type: "array",
                    items: { type: "string" },
                    description: "App launch command passed after `--`. Omit to use the config `start` command.",
                },
                args: {
                    type: "array",
                    items: { type: "string" },
                    description: "Extra drill test arguments, e.g. [\"--grep-invert=@visual\"].",
                },
            },
        },
    },
};

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

function configure(state, input) {
    state.workspace = workspaceFor(input);
    state.cli = cliFor(input);
    state.run = input?.run ?? {};
    state.list = null;
    state.listSignature = null;
    state.listError = null;
    return state;
}

export default {
    id: "auditaur-drill-explorer",
    displayName: "Auditaur Drill Explorer",
    description: "Browse Auditaur drill test suites with each test's latest result, recent history, and flakiness; run or cancel drills.",
    inputSchema: openSchema,
    title: "Drill tests",
    page: "explorer",
    view: {
        filter: field.text().about("Show only tests whose `suite > test @tags` contain every word."),
        expanded: field.ids().about("Test ids whose steps and last error are expanded."),
    },
    create: (input, view) => configure({ view, launch: null }, input),
    reconfigure(state, input) {
        if (state.starting || (state.launch && !state.launch.exited)) {
            throw new DrillError("run_in_progress", "cancel or finish the current run before reconfiguring this canvas");
        }
        configure(state, input);
    },
    describe: async (state) => ({ shown: visibleTests(await snapshot(state)) }),
    routes: (state) => ({
        "GET /state": (url) => snapshot(state, url.searchParams.has("refresh")),
        "POST /run": async (_url, req) => startRun(state, await readBody(req)),
        "POST /cancel": () => cancelRun(state),
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
        {
            name: "cancel",
            description: "Stop the drill run in progress; it finishes as interrupted.",
            handler: (state) => cancelRun(state),
        },
    ],
};