// Snapshots page: review a run's ARIA and screenshot mismatches and accept new baselines.

import { RUN_ID } from "../../lib/paths.mjs";
import { DrillError } from "../../lib/errors.mjs";
import { runIdProperty } from "../../lib/schemas.mjs";
import { readBody } from "../../lib/server.mjs";
import { field } from "../../lib/view-state.mjs";
import { acceptSnapshots, snapshotsSummary, snapshotsView } from "./view.mjs";

const acceptSchema = {
    type: "object",
    additionalProperties: false,
    required: ["ids"],
    properties: {
        runId: { ...runIdProperty, description: "Run the ids belong to; defaults to the routed run." },
        ids: { type: "array", minItems: 1, items: { type: "string" }, description: "Snapshot ids from get_snapshots." },
    },
};

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
    page: "snapshots",
    view: {
        mode: field.choice(["side", "slider", "diff"], "side").about("How screenshot mismatches compare expected and actual."),
        split: field.range(0, 100, 50).about("Slider mode: percent of the width showing expected (left) over actual."),
    },
    describe: async (state) => ({ runId: (await snapshotsView(state)).run?.runId ?? null }),
    routes: (state) => ({
        "GET /state": () => snapshotsView(state),
        "POST /accept": async (_url, req) => accept(state, await readBody(req)),
    }),
    actions: [
        {
            name: "get_snapshots",
            description: "The routed run's snapshot mismatches (matched snapshot steps are not listed): baseline path, status (changed, missing, accepted, unstable, unreadable, noActual), message, and attachment paths; plus its named screenshots.",
            readOnly: true,
            handler: async (state) => snapshotsSummary(await snapshotsView(state)),
        },
        {
            name: "accept_snapshots",
            description: "Overwrite the baselines of the given snapshot ids with the run's actual output. Review the diff first; the run must have finished.",
            inputSchema: acceptSchema,
            handler: accept,
        },
    ],
};