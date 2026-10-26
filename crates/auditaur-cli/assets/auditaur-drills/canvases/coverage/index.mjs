// Coverage canvas: which IPC commands and events a drill run's tests exercised, and which they missed.

import { RUN_ID, cliFor, workspaceFor } from "../../lib/paths.mjs";
import { DrillError } from "../../lib/errors.mjs";
import { cwdProperty, runIdProperty } from "../../lib/schemas.mjs";
import { readBody } from "../../lib/server.mjs";
import { listAppStores } from "../../lib/apps.mjs";
import { field } from "../../lib/view-state.mjs";
import { coverageSummary, coverageView } from "./view.mjs";

const dbsProperty = {
    type: "array",
    items: { type: "string", minLength: 1 },
    description: "Telemetry stores (`telemetry.sqlite`) whose commands and events count as the app's surface, so unexercised ones are listed. See list_app_stores.",
};

const inputSchema = {
    type: "object",
    additionalProperties: false,
    properties: {
        cwd: cwdProperty,
        cli: { type: "string", description: "Path to the auditaur CLI. Defaults to $AUDITAUR_BIN, then `auditaur` on PATH." },
        runId: runIdProperty,
        dbs: dbsProperty,
    },
};

const selectSchema = { type: "object", additionalProperties: false, properties: { runId: runIdProperty } };
const sourcesSchema = { type: "object", additionalProperties: false, required: ["dbs"], properties: { dbs: dbsProperty } };

function select(state, input) {
    if (input.runId != null && !RUN_ID.test(input.runId)) throw new DrillError("invalid_run", "not a drill run id");
    state.runId = input.runId ?? null;
    return { runId: state.runId };
}

function checkDbs(dbs) {
    if (!Array.isArray(dbs) || !dbs.every((db) => typeof db === "string" && db.length)) {
        throw new DrillError("invalid_dbs", "dbs is a list of telemetry store paths");
    }
    return [...new Set(dbs)];
}

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

function configure(state, input) {
    state.workspace = workspaceFor(input);
    state.cli = cliFor(input);
    state.dbs = checkDbs(input?.dbs ?? []);
    state.trusted = new Set(state.dbs);
    state.cache = null;
    select(state, input ?? {});
    return state;
}

export default {
    id: "auditaur-drill-coverage",
    displayName: "Auditaur Drill Coverage",
    description: "See which Tauri IPC commands and events a drill run's tests exercised, per test, and which ones seen in app telemetry no test touched.",
    inputSchema,
    title: "Drill coverage",
    page: "coverage",
    view: {
        tab: field.choice(["commands", "events", "tests", "map"], "commands").about("Which coverage list the page shows."),
        missedOnly: field.flag(false).about("Show only commands, events, and tests no test exercised."),
    },
    create: (input, view) => configure({ view }, input),
    reconfigure: configure,
    navigate: (state, { runId }) => select(state, { runId }),
    describe: async (state) => ({ runId: (await coverageView(state)).run?.runId ?? null, dbs: state.dbs }),
    routes: (state) => ({
        "GET /state": (url) => coverageView(state, { fresh: url.searchParams.has("refresh") }),
        "GET /apps": () => listAppStores(state),
        "POST /select": async (_url, req) => select(state, await readBody(req)),
        "POST /sources": async (_url, req) => pickStores(state, await readBody(req)),
    }),
    actions: [
        {
            name: "get_coverage",
            description: "Totals, unexercised commands and events, failing commands, and tests without complete telemetry for the shown run. Always re-reads the run and stores.",
                        readOnly: true,
                        handler: async (state) => coverageSummary(await coverageView(state, { fresh: true })),
        },
        {
            name: "select",
            description: "Show another run. Omit runId to follow the newest run.",
            inputSchema: selectSchema,
            handler: async (state, input) => {
                select(state, input);
                return coverageSummary(await coverageView(state));
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
            description: "Compare against these telemetry stores; an empty list shows only what the run exercised.",
            inputSchema: sourcesSchema,
            handler: async (state, input) => {
                state.dbs = checkDbs(input.dbs);
                for (const db of state.dbs) state.trusted.add(db);
                return coverageSummary(await coverageView(state));
            },
        },
    ],
};
