// Opening one drill canvas from another: a page's "open in" link or the agent's `open_in` action.
//
// Reopening an instance id only focuses it (the runtime does not call `open()` again), so a target
// already open in this provider is pointed at the run or test here, then focused. Otherwise a new
// panel opens with that input. Either way the same panel is reused for repeated jumps.

import { DrillError } from "./errors.mjs";
import { createHash } from "node:crypto";
import { RUN_ID, workspaceKey } from "./paths.mjs";
import { readRun } from "./runs.mjs";
import { readBody } from "./server.mjs";

// Link name -> canvas id. Only canvases that show one run (or one test of it) are targets.
export const TARGETS = {
    run: "auditaur-drill-run",
    trace: "auditaur-drill-trace",
    snapshots: "auditaur-drill-snapshots",
    coverage: "auditaur-drill-coverage",
};

export const openInSchema = {
    type: "object",
    additionalProperties: false,
    required: ["to"],
    properties: {
        to: { type: "string", enum: Object.keys(TARGETS), description: "Which canvas to open." },
        runId: { type: "string", description: "Run to show; omit for the newest run." },
        testId: { type: "string", description: "Test to show (trace only); omit for the first failed test." },
    },
};

function target(request) {
    const { to, runId, testId } = request ?? {};
    if (!Object.hasOwn(TARGETS, to)) {
        throw new DrillError("invalid_target", `open in one of: ${Object.keys(TARGETS).join(", ")}`);
    }
    for (const [name, value] of [["runId", runId], ["testId", testId]]) {
        if (value != null && typeof value !== "string") throw new DrillError("invalid_target", `${name} is a string`);
    }
    if (testId != null && to !== "trace") throw new DrillError("invalid_target", "only the trace canvas shows one test");
    return { to, selection: { ...(runId != null && { runId }), ...(testId != null && { testId }) } };
}

// One panel per target and workspace, so jumps from two workspaces never repoint each other.
const panelId = (to, workspace) => `auditaur-${to}-${createHash("sha256").update(workspaceKey(workspace)).digest("hex").slice(0, 10)}`;

export class Navigator {
    #panels;
    #canvases;
    #rpc = null;

    constructor(panels, canvases) {
        this.#panels = panels;
        this.#canvases = new Map(canvases.map((canvas) => [canvas.id, canvas]));
    }

    // The session's RPC client; until it is bound (or if the runtime lacks canvas RPC) links refuse.
    bind(rpc) {
        this.#rpc = rpc?.canvas?.open ? rpc : null;
    }

    async open(from, request) {
        const { to, selection } = target(request);
        const canvas = this.#canvases.get(TARGETS[to]);
        if (!this.#rpc) throw new DrillError("navigation_unavailable", "this session cannot open canvases from a canvas");
        if (selection.runId != null && RUN_ID.test(selection.runId) && !(await readRun(from, selection.runId))) {
            throw new DrillError("not_found", `no drill run ${selection.runId}`);
        }
        const existing = this.#panels.find(canvas.id, from.workspace);
        // Validate and apply before focusing, so a bad id never moves the user to another panel.
        if (existing) this.#point(canvas, existing.state, selection);
        const instanceId = existing?.instanceId ?? panelId(to, from.workspace);
        try {
            await this.#rpc.canvas.open({
                canvasId: canvas.id,
                instanceId,
                ...(!existing && { input: { cwd: from.workspace, ...selection } }),
            });
        } catch (error) {
            throw new DrillError("navigation_failed", `the host refused: ${error?.message ?? error}`);
        }
        // A panel the runtime already had (e.g. restored after a reload) only got focused.
        const opened = existing ?? this.#panels.find(canvas.id, from.workspace);
        if (!existing && opened) this.#point(canvas, opened.state, selection);
        return { canvasId: canvas.id, instanceId: opened?.instanceId ?? instanceId, ...selection };
    }

    #point(canvas, state, selection) {
        canvas.navigate(state, selection);
        state.view.changed();
    }
}

// `POST /open` on every panel: the page's "open in" links.
export const navigationRoutes = (navigator) => (state) => ({
    "POST /open": async (_url, req) => navigator.open(state, await readBody(req)),
});

// The same jump for the agent, so every link in a page has an action.
export const navigationAction = (navigator) => ({
    name: "open_in",
    description: "Open, or focus and point, the run, trace, snapshots, or coverage canvas at a run (and, for trace, a test). Same as the panel's ↗ links.",
    inputSchema: openInSchema,
    handler: (state, input) => navigator.open(state, input),
});
