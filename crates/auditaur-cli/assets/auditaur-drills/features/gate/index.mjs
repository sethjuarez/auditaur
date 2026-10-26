// Gate page: one human gate of an `auditaur drill` run (outside a drill suite), opened from the
// request list_pending_human_gates returns. Gates of a drill suite run show on the run page.

import { DrillError } from "../../lib/errors.mjs";
import { publishAnswer, queuePaths } from "../../lib/gate-queue.mjs";
import { readBody } from "../../lib/server.mjs";
import { gateSummary, gateView } from "./view.mjs";

const answerProperties = {
    choiceId: { type: "string", description: "One of the gate's choices; omit when it has none." },
    inputs: {
        type: "array",
        items: {
            type: "object",
            additionalProperties: false,
            required: ["id", "value"],
            properties: { id: { type: "string" }, value: { type: "string" } },
        },
    },
};

function answerGate(state, body, responder) {
    if (!state.gate) throw new DrillError("no_gate", "no gate is shown; use show_gate first");
    return publishAnswer({ requestPath: state.gate, body, responder });
}

export default {
    page: "gate",
    view: {},
    describe: async (state) => gateSummary(await gateView(state)),
    routes: (state) => ({
        "GET /state": () => gateView(state),
        "POST /answer": async (_url, req) => answerGate(state, await readBody(req), "human-confirmed-via-drills-canvas"),
    }),
    actions: [
        {
            name: "show_gate",
            description: "Show a pending human gate of an `auditaur drill` run on the gate page and switch to it. Pass the requestPath of an entry from the list_pending_human_gates MCP tool.",
            inputSchema: {
                type: "object",
                additionalProperties: false,
                required: ["requestPath"],
                properties: { requestPath: { type: "string", description: "The request's absolute path." } },
            },
            handler: async (state, input, { navigate }) => {
                state.gate = queuePaths(input.requestPath).requestPath;
                navigate("/gate");
                return gateView(state);
            },
        },
        {
            name: "get_gate",
            description: "The gate the gate page shows: its instructions, choices, inputs, and whether it is still waiting.",
            readOnly: true,
            handler: (state) => gateView(state),
        },
        {
            name: "answer_gate",
            description: "Answer the gate the gate page shows, only as the user asked. For gates of a drill suite run, use respond_gate.",
            inputSchema: { type: "object", additionalProperties: false, properties: answerProperties },
            handler: (state, input) => answerGate(state, input, "agent-via-drills-canvas"),
        },
    ],
};
