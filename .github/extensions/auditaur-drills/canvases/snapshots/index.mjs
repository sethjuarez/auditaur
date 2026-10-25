// Snapshots canvas: review a run's ARIA and screenshot mismatches and accept new baselines.

import { RUN_ID, workspaceFor } from "../../lib/paths.mjs";
import { DrillError } from "../../lib/errors.mjs";
import { cwdProperty, runIdProperty } from "../../lib/schemas.mjs";
import { readBody } from "../../lib/server.mjs";
import { serveAttachment } from "../../lib/attachments.mjs";
import { field } from "../../lib/view-state.mjs";
import { acceptSnapshots, snapshotsSummary, snapshotsView } from "./view.mjs";

const inputSchema = {
    type: "object",
    additionalProperties: false,
    properties: { cwd: cwdProperty, runId: runIdProperty },
};

const selectSchema = { type: "object", additionalProperties: false, properties: { runId: runIdProperty } };

const acceptSchema = {
    type: "object",
    additionalProperties: false,
    required: ["ids"],
    properties: {
        runId: { ...runIdProperty, description: "Run the ids belong to; defaults to the shown run." },
        ids: { type: "array", minItems: 1, items: { type: "string" }, description: "Snapshot ids from get_snapshots." },
    },
};

function select(state, input) {
    if (input.runId != null && !RUN_ID.test(input.runId)) throw new DrillError("invalid_run", "not a drill run id");
    state.runId = input.runId ?? null;
    return { runId: state.runId };
}

function configure(state, input) {
    state.workspace = workspaceFor(input);
    select(state, input ?? {});
    return state;
}

async function accept(state, input) {
    const ids = input.ids;
    if (!Array.isArray(ids) || !ids.length || !ids.every((id) => typeof id === "string")) {
        throw new DrillError("invalid_ids", "ids is a non-empty list of snapshot ids");
    }
    const runId = input.runId ?? state.runId ?? (await snapshotsView(state)).run?.runId;
    if (!runId || !RUN_ID.test(runId)) throw new DrillError("invalid_run", "no run to accept snapshots from");
    return acceptSnapshots(state, runId, ids);
}

export default {
    id: "auditaur-drill-snapshots",
    displayName: "Auditaur Drill Snapshots",
    description: "Review a drill run's ARIA and screenshot snapshot mismatches (expected, actual, diff) and accept new baselines.",
    inputSchema,
    title: "Drill snapshots",
    page: "snapshots",
    components: ["compare", "links"],
    view: {
        mode: field.choice(["side", "slider", "diff"], "side").about("How screenshot mismatches compare expected and actual."),
    },
    create: (input, view) => configure({ view }, input),
    reconfigure: configure,
    navigate: (state, { runId }) => select(state, { runId }),
    describe: async (state) => ({ runId: (await snapshotsView(state)).run?.runId ?? null }),
    routes: (state) => ({
        "GET /state": () => snapshotsView(state),
        "GET /file": (url) => serveAttachment(state, url),
        "POST /select": async (_url, req) => select(state, await readBody(req)),
        "POST /accept": async (_url, req) => accept(state, await readBody(req)),
    }),
    actions: [
        {
            name: "get_snapshots",
            description: "The shown run's snapshot results: baseline path, status (changed, missing, accepted, unstable, unreadable, noActual), message, and attachment paths.",
            readOnly: true,
            handler: async (state) => snapshotsSummary(await snapshotsView(state)),
        },
        {
            name: "select",
            description: "Show another run. Omit runId to follow the newest run.",
            inputSchema: selectSchema,
            handler: async (state, input) => {
                select(state, input);
                return snapshotsSummary(await snapshotsView(state));
            },
        },
        {
            name: "accept",
            description: "Overwrite the baselines of the given snapshot ids with the run's actual output. Review the diff first; the run must have finished.",
            inputSchema: acceptSchema,
            handler: accept,
        },
    ],
};
