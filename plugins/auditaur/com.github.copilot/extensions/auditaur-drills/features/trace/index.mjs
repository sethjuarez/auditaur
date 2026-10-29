// Trace page: step through one test attempt with screenshots, DOM, and telemetry lined up by time.

import { field } from "../../lib/view-state.mjs";
import { TELEMETRY_KINDS } from "../../lib/telemetry.mjs";
import { traceSummary, traceView } from "./view.mjs";

export default {
    page: "trace",
    view: {
        step: field.index(["auto", "all"], "auto").about(
            "Zero-based step to focus; `auto` focuses the failing step, `all` the whole attempt. Showing another attempt resets it to `auto`.",
        ),
        kinds: field.ids(TELEMETRY_KINDS).about("Telemetry kinds shown on the timeline and in the table."),
    },
    // Resolving the shown attempt rescopes the step, so writes land on the attempt actually shown.
    sync: (state) => traceView(state),
    describe: async (state) => {
        const { selected } = await traceView(state);
        const step = selected?.steps[selected.step] ?? null;
        return { testId: selected?.key ?? null, attempt: selected?.attempt ?? null, step: step && { index: step.index, title: step.title, status: step.status } };
    },
    routes: (state) => ({
        "GET /state": () => traceView(state),
    }),
    actions: [
        {
            name: "get_trace",
            description: "The routed attempt's steps with timing, errors, attachments, and the telemetry recorded during each step (first failed test of the newest run when the route names none).",
            readOnly: true,
            handler: async (state) => traceSummary(await traceView(state)),
        },
    ],
};