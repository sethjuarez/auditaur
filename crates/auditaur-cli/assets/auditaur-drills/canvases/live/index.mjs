// Run canvas: watch a drill run step by step and answer its human gates.

import { RUN_ID, workspaceFor } from "../../lib/paths.mjs";
import { pickRun } from "../../lib/runs.mjs";
import { cancelRun } from "../../lib/launcher.mjs";
import { respondGate } from "../../lib/gates.mjs";
import { DrillError } from "../../lib/errors.mjs";
import { cwdProperty, runIdProperty } from "../../lib/schemas.mjs";
import { readBody } from "../../lib/server.mjs";
import { field } from "../../lib/view-state.mjs";
import { liveSummary, liveView } from "./view.mjs";

const inputSchema = {
    type: "object",
    additionalProperties: false,
    properties: { cwd: cwdProperty, runId: runIdProperty },
};

const respondSchema = {
    type: "object",
    additionalProperties: false,
    required: ["requestId"],
    properties: {
        requestId: { type: "string" },
        choiceId: { type: "string" },
        inputs: {
            type: "array",
            items: {
                type: "object",
                additionalProperties: false,
                required: ["id", "value"],
                properties: { id: { type: "string" }, value: { type: "string" } },
            },
        },
    },
};

const followSchema = {
    type: "object",
    additionalProperties: false,
    properties: { runId: { ...runIdProperty, description: "Show this run; omit to follow the newest run." } },
};

function configure(state, input) {
    state.workspace = workspaceFor(input);
    state.runId = input?.runId ?? null;
    return state;
}

async function cancelShown(state) {
    const { run } = await pickRun(state);
    if (!run || run.status !== "running") {
        throw new DrillError("no_active_run", "the shown run is not in progress");
    }
    return cancelRun(state, run.runId);
}

function follow(state, runId) {
    if (runId != null && !RUN_ID.test(runId)) {
        throw new DrillError("invalid_run", "not a drill run id");
    }
    state.runId = runId;
    return { pinned: state.runId };
}

export default {
    id: "auditaur-drill-run",
    displayName: "Auditaur Drill Run",
    description: "Watch an Auditaur drill run live, step by step, and answer its human gates; follows the newest run unless given a runId.",
    inputSchema,
    title: "Drill run",
    page: "live",
    view: {
        expanded: field.toggles().about(
            "Open (true) or closed (false) overrides by test id, or `hooks:<suite id>` for suite hooks. Without one, running and failed tests are open.",
        ),
    },
    create: (input, view) => configure({ view }, input),
    reconfigure: configure,
    describe: async (state) => {
        const { pinned, run } = await liveView(state);
        return { following: pinned ? "pinned" : "newest", runId: run?.runId ?? null };
    },
    routes: (state) => ({
        "GET /state": () => liveView(state),
        "POST /gate": async (_url, req) => respondGate(state, await readBody(req)),
        "POST /cancel": () => cancelShown(state),
        "POST /follow": async (_url, req) => follow(state, (await readBody(req)).runId ?? null),
    }),
    actions: [
        {
            name: "get_status",
            description: "The shown run's status, totals, running and failed tests, and gates waiting for an answer.",
            readOnly: true,
            handler: async (state) => liveSummary(await liveView(state)),
        },
        {
            name: "follow",
            description: "Show one run, or omit runId to follow the newest run.",
            inputSchema: followSchema,
            handler: async (state, input) => {
                follow(state, input.runId ?? null);
                return liveSummary(await liveView(state));
            },
        },
        {
            name: "respond_gate",
            description: "Answer a gate the shown run is waiting on. Omit choiceId for gates without choices.",
            inputSchema: respondSchema,
            handler: (state, input) => respondGate(state, input),
        },
        {
            name: "cancel",
            description: "Stop the shown run; it finishes as interrupted.",
            handler: (state) => cancelShown(state),
        },
    ],
};