// What a panel is opened with, and the state every page of it shares: the workspace, the CLI, how
// runs launch, the telemetry stores coverage compares against, and the selection from the route.

import { DrillError } from "./errors.mjs";
import { queuePaths } from "./gate-queue.mjs";
import { cliFor, workspaceFor } from "./paths.mjs";
import { cwdProperty } from "./schemas.mjs";

export const dbsProperty = {
    type: "array",
    items: { type: "string", minLength: 1 },
    description: "Telemetry stores (`telemetry.sqlite`) whose commands and events count as the app's surface, so coverage lists unexercised ones. See list_app_stores.",
};

export const cliProperty = {
    type: "string",
    description: "Path to the auditaur CLI. Defaults to $AUDITAUR_BIN, then `auditaur` on PATH.",
};

export const openSchema = {
    type: "object",
    additionalProperties: false,
    properties: {
        cwd: cwdProperty,
        cli: cliProperty,
        run: {
            type: "object",
            additionalProperties: false,
            description: "How the run buttons launch `drill test`.",
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
        dbs: dbsProperty,
        route: {
            type: "string",
            maxLength: 600,
            description: "Page to show first, e.g. `/trace?run=<runId>&test=<testId>`. Pages: /tests, /run, /trace, /snapshots, /coverage, /gate. Only used by a new panel; use the `navigate` action on an open one.",
        },
        gate: {
            type: "object",
            additionalProperties: true,
            required: ["requestPath"],
            description: "A pending human gate of an `auditaur drill` run to show on the gate page: an entry from the list_pending_human_gates MCP tool (only requestPath is read). The panel opens on /gate.",
            properties: { requestPath: { type: "string", description: "The request's absolute path." } },
        },
    },
};

export function checkDbs(dbs) {
    if (!Array.isArray(dbs) || !dbs.every((db) => typeof db === "string" && db.length)) {
        throw new DrillError("invalid_dbs", "dbs is a list of telemetry store paths");
    }
    return [...new Set(dbs)];
}

// The shared state for open `input`. `trusted` are stores the page may pick without discovery.
export function sharedState(input) {
    const dbs = checkDbs(input?.dbs ?? []);
    return {
        workspace: workspaceFor(input),
        cli: cliFor(input),
        run: input?.run ?? {},
        dbs,
        trusted: new Set(dbs),
        // The gate request the gate page shows.
        gate: input?.gate ? queuePaths(input.gate.requestPath).requestPath : null,
        // Selection, from the route.
        runId: null,
        testId: null,
        attempt: null,
        // Caches and the run this panel launched.
        list: null,
        listSignature: null,
        listError: null,
        cache: null,
        launch: null,
        starting: false,
    };
}

// What a new panel opened from this one inherits: everything but the route and gate.
export const inheritedInput = (input) => {
    const { route: _route, gate: _gate, ...rest } = input ?? {};
    return rest;
};
