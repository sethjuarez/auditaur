// Coverage page: which IPC commands and events a drill run's tests exercised, and which they missed.

import { DrillError } from "../../lib/errors.mjs";
import { checkDbs, dbsProperty } from "../../lib/config.mjs";
import { readBody } from "../../lib/server.mjs";
import { listAppStores } from "../../lib/apps.mjs";
import { field } from "../../lib/view-state.mjs";
import { runIdProperty } from "../../lib/schemas.mjs";
import { coverageSummary, coverageView } from "./view.mjs";

const sourcesSchema = { type: "object", additionalProperties: false, required: ["dbs"], properties: { dbs: dbsProperty } };

// The page may only pick stores `auditaur apps` discovered, or ones the agent chose.
async function pickStores(state, input) {
    const dbs = checkDbs(input.dbs);
    const { stores } = await listAppStores(state);
    const allowed = new Set([...stores.map((store) => store.db), ...state.trusted]);
    const unknown = dbs.filter((db) => !allowed.has(db));
    if (unknown.length) throw new DrillError("unknown_store", `not a discovered app store: ${unknown.join(", ")}`);
    state.dbs = dbs;
    return { dbs };
}

export default {
    page: "coverage",
    view: {
        tab: field.choice(["commands", "events", "tests", "map"], "commands").about("Which coverage list the page shows."),
        missedOnly: field.flag(false).about("Show only commands, events, and tests no test exercised."),
    },
    describe: async (state) => ({ runId: (await coverageView(state)).run?.runId ?? null, dbs: state.dbs }),
    routes: (state) => ({
        "GET /state": (url) => coverageView(state, { fresh: url.searchParams.has("refresh") }),
        "GET /apps": () => listAppStores(state),
        "POST /sources": async (_url, req) => pickStores(state, await readBody(req)),
    }),
    actions: [
        {
            name: "get_coverage",
            description: "Totals, unexercised commands and events, failing commands, and tests without complete telemetry for a run: runId, else the routed one. Always re-reads the run and stores; does not change the page.",
            readOnly: true,
            inputSchema: { type: "object", additionalProperties: false, properties: { runId: runIdProperty } },
            handler: async (state, input) => {
                // A copy, so asking about another run leaves the page's run and cache alone.
                const target = input.runId ? { ...state, runId: input.runId, cache: null, coverageLoad: null } : state;
                const view = await coverageView(target, { fresh: true });
                if (input.runId && !view.run) throw new DrillError("invalid_run", `run ${input.runId} was not found`);
                return coverageSummary(view);
            },
        },
        {
            name: "list_app_stores",
            description: "Auditaur app sessions on this machine whose telemetry stores can be compared against.",
            readOnly: true,
            handler: (state) => listAppStores(state),
        },
        {
            name: "set_sources",
            description: "Compare coverage against these telemetry stores; an empty list shows only what the run exercised.",
            inputSchema: sourcesSchema,
            handler: async (state, input) => {
                state.dbs = checkDbs(input.dbs);
                for (const db of state.dbs) state.trusted.add(db);
                return coverageSummary(await coverageView(state));
            },
        },
    ],
};