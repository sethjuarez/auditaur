// Extension: auditaur-drills
// Canvases over Auditaur drill tests. They read `auditaur drill ... --json` output and the
// versioned run folders under `.auditaur/drill-runs/`; they never compute results themselves.
//
// This file only wires canvas definitions (`canvases/*/index.mjs`) to the SDK. It is the one module
// that imports the SDK; everything else throws `DrillError`, which is translated here.

import { joinSession, createCanvas, CanvasError } from "@github/copilot-sdk/extension";
import { DrillError } from "./lib/errors.mjs";
import { Panels } from "./lib/panels.mjs";
import { sweepOrphanLogs } from "./lib/processes.mjs";
import explorer from "./canvases/explorer/index.mjs";
import live from "./canvases/live/index.mjs";
import trace from "./canvases/trace/index.mjs";
import snapshots from "./canvases/snapshots/index.mjs";
import coverage from "./canvases/coverage/index.mjs";

const panels = new Panels();

function surfaced(fn) {
    return async (...args) => {
        try {
            return await fn(...args);
        } catch (error) {
            if (error instanceof DrillError) throw new CanvasError(error.code, error.message);
            throw error;
        }
    };
}

function toCanvas(canvas) {
    return createCanvas({
        id: canvas.id,
        displayName: canvas.displayName,
        description: canvas.description,
        inputSchema: canvas.inputSchema,
        actions: canvas.actions.map((action) => ({
            name: action.name,
            description: action.description,
            ...(action.inputSchema && { inputSchema: action.inputSchema }),
            handler: surfaced((ctx) => action.handler(panels.state(ctx.instanceId), ctx.input ?? {})),
        })),
        open: surfaced(async (ctx) => {
            const panel = await panels.open(canvas, ctx.instanceId, ctx.input);
            return { title: canvas.title, url: panel.url };
        }),
        onClose: (ctx) => panels.close(ctx.instanceId),
    });
}

sweepOrphanLogs();

await joinSession({ canvases: [explorer, live, trace, snapshots, coverage].map(toCanvas) });