// Trace canvas: step through one test attempt with screenshots, DOM, and telemetry lined up by time.

import { RUN_ID, TEST_ID, workspaceFor } from "../../lib/paths.mjs";
import { DrillError } from "../../lib/errors.mjs";
import { cwdProperty, runIdProperty } from "../../lib/schemas.mjs";
import { readBody } from "../../lib/server.mjs";
import { serveAttachment } from "../../lib/attachments.mjs";
import { field } from "../../lib/view-state.mjs";
import { TELEMETRY_KINDS } from "../../lib/telemetry.mjs";
import { traceSummary, traceView } from "./view.mjs";

const HOOKS_ID = /^hooks:[A-Za-z0-9_-]{1,128}$/;

const attemptProperty = { type: "integer", minimum: 0, description: "Zero-based attempt; defaults to the last one." };

const inputSchema = {
    type: "object",
    additionalProperties: false,
    properties: {
        cwd: cwdProperty,
        runId: runIdProperty,
        testId: { type: "string", pattern: TEST_ID.source, description: "Test id from run.json; defaults to the first failed test." },
        attempt: attemptProperty,
    },
};

const selectSchema = {
    type: "object",
    additionalProperties: false,
    properties: { runId: runIdProperty, testId: { type: "string" }, attempt: attemptProperty },
};

function select(state, input) {
    if (input.runId != null && !RUN_ID.test(input.runId)) throw new DrillError("invalid_run", "not a drill run id");
    if (input.testId != null && !TEST_ID.test(input.testId) && !HOOKS_ID.test(input.testId)) {
        throw new DrillError("invalid_test", "not a drill test id");
    }
    if (input.attempt != null && !(Number.isInteger(input.attempt) && input.attempt >= 0)) {
        throw new DrillError("invalid_attempt", "attempt is a zero-based integer");
    }
    state.runId = input.runId ?? null;
    state.testId = input.testId ?? null;
    state.attempt = input.attempt ?? null;
    state.view.reset("step");
    return { runId: state.runId, testId: state.testId, attempt: state.attempt };
}

function configure(state, input) {
    state.workspace = workspaceFor(input);
    select(state, input ?? {});
    return state;
}

export default {
    id: "auditaur-drill-trace",
    displayName: "Auditaur Drill Trace",
    description: "Step through one drill test attempt: a step timeline with screenshots, DOM, IPC, events, logs, and errors lined up by time.",
    inputSchema,
    title: "Drill trace",
    page: "trace",
    view: {
        step: field.index(["auto", "all"], "auto").about(
            "Zero-based step to focus; `auto` focuses the failing step, `all` the whole attempt. Selecting another test resets it to `auto`.",
        ),
        kinds: field.ids(TELEMETRY_KINDS).about("Telemetry kinds shown on the timeline and in the table."),
    },
    create: (input, view) => configure({ view }, input),
    reconfigure: configure,
    // Resolving the shown attempt rescopes the step, so writes land on the attempt actually shown.
    sync: (state) => traceView(state),
    describe: async (state) => {
        const { selected } = await traceView(state);
        const step = selected?.steps[selected.step] ?? null;
        return { testId: selected?.key ?? null, attempt: selected?.attempt ?? null, step: step && { index: step.index, title: step.title, status: step.status } };
    },
    routes: (state) => ({
        "GET /state": () => traceView(state),
        "GET /file": (url) => serveAttachment(state, url),
        "POST /select": async (_url, req) => select(state, await readBody(req)),
    }),
    actions: [
        {
            name: "get_trace",
            description: "The shown attempt's steps with timing, errors, attachments, and the telemetry recorded during each step.",
            readOnly: true,
            handler: async (state) => traceSummary(await traceView(state)),
        },
        {
            name: "select",
            description: "Show another run, test, or attempt. Omit runId to follow the newest run.",
            inputSchema: selectSchema,
            handler: async (state, input) => {
                select(state, input);
                return traceSummary(await traceView(state));
            },
        },
    ],
};