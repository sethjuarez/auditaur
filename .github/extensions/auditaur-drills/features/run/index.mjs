// Run page: watch a drill run step by step and answer its human gates.

import { respondGate } from "../../lib/gates.mjs";
import { readBody } from "../../lib/server.mjs";
import { field } from "../../lib/view-state.mjs";
import { liveSummary, liveView } from "./view.mjs";

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

export default {
    page: "run",
    view: {
        expanded: field.toggles().about(
            "Open (true) or closed (false) overrides by test id, or `hooks:<suite id>` for suite hooks. Without one, running and failed tests are open.",
        ),
    },
    describe: async (state) => {
        const { pinned, run } = await liveView(state);
        return { following: pinned ? "pinned" : "newest", runId: run?.runId ?? null };
    },
    routes: (state) => ({
        "GET /state": () => liveView(state),
        "POST /gate": async (_url, req) => respondGate(state, await readBody(req)),
    }),
    actions: [
        {
            name: "get_status",
            description: "The routed run's status, totals, running and failed tests, and gates waiting for an answer (newest run when the route names none).",
            readOnly: true,
            handler: async (state) => liveSummary(await liveView(state)),
        },
        {
            name: "respond_gate",
            description: "Answer a gate the routed run is waiting on. Omit choiceId for gates without choices.",
            inputSchema: respondSchema,
            handler: (state, input) => respondGate(state, input),
        },
    ],
};