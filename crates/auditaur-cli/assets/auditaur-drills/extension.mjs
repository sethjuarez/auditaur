// Extension: auditaur-drills
// One canvas over Auditaur drill tests: a routed app with Tests, Run, Trace, Snapshots, and Coverage
// pages. It reads `auditaur drill ... --json` output and the versioned run folders under
// `.auditaur/drill-runs/`; it never computes results itself.
//
// This file only wires the app to the SDK. It is the one module that imports the SDK; everything
// else throws `DrillError`, which is translated here.

import { joinSession, createCanvas, CanvasError } from "@github/copilot-sdk/extension";
import { canvasActions, runAction } from "./lib/actions.mjs";
import { reportFailedPages } from "./lib/build-info.mjs";
import { openSchema } from "./lib/config.mjs";
import { agentMessage, DrillError } from "./lib/errors.mjs";
import { loadFeatures } from "./lib/features.mjs";
import { PanelStore } from "./lib/panel-store.mjs";
import { CANVAS_ID, Panels } from "./lib/panels.mjs";
import { DEFERS_TO_PROJECT, projectCopy, REPO_ROOT } from "./lib/paths.mjs";
import { sweepOrphanLogs } from "./lib/processes.mjs";

function surfaced(fn) {
    return async (...args) => {
        try {
            return await fn(...args);
        } catch (error) {
            if (error instanceof DrillError) throw new CanvasError(error.code, agentMessage(error));
            throw error;
        }
    };
}

async function serve() {
    const { features, failed } = await loadFeatures();
    reportFailedPages(failed);
    const panels = new Panels({ features, store: new PanelStore() });

    const toSdkAction = (action) => ({
        name: action.name,
        description: action.description,
        ...(action.inputSchema && { inputSchema: action.inputSchema }),
        handler: surfaced(async (ctx) => {
            const { panel, url } = await panels.get(ctx.instanceId);
            return runAction(action, panel, ctx.input, { url });
        }),
    });

    const canvas = createCanvas({
        id: CANVAS_ID,
        displayName: "Auditaur Drills",
        description:
            "Auditaur drill tests as one app: browse suites and results, run drills and watch them live (answering human gates), step through a test's trace, review snapshot mismatches, and see IPC/event coverage. Also answers a pending `auditaur drill` human gate (open with input.gate). Use the navigate action to change pages.",
        inputSchema: openSchema,
        actions: canvasActions(features, (from, route) => panels.openNew(from, route)).map(toSdkAction),
        open: surfaced(async (ctx) => {
            const { url } = await panels.open(ctx.instanceId, ctx.input);
            return { title: "Drills", url };
        }),
        onClose: (ctx) => panels.close(ctx.instanceId),
    });

    sweepOrphanLogs();

    const session = await joinSession({ canvases: [canvas] });
    panels.bind({ workspacePath: session.workspacePath, rpc: session.rpc });
}

// A plugin copy declares no canvas when the repository commits its own, so the id has one provider.
if (DEFERS_TO_PROJECT) {
    const session = await joinSession({});
    await session.log(
        `Auditaur Drills: using this repository's committed canvas (${projectCopy(REPO_ROOT)}); the installed copy is idle.`,
        { level: "info", ephemeral: true },
    );
} else {
    await serve();
}
