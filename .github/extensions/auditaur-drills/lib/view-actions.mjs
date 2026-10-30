// Agent actions that move the panel between pages and read or change a page's view settings.

import { currentView } from "./current-view.mjs";
import { PAGES, formatRoute } from "./route.mjs";
import { runIdProperty } from "./schemas.mjs";

const pageProperty = { type: "string", enum: Object.keys(PAGES) };

function viewFieldsHelp(features) {
    return features.map((feature) => `${feature.page}: ${Object.keys(feature.view ?? {}).join(", ") || "none"}`).join("; ");
}

export function viewActions(features, openPanel) {
    return [
        {
            name: "navigate",
            description: "Show a page of this panel: tests, run (live progress and gates), trace (one test attempt), snapshots, coverage, or gate (one `auditaur drill` gate; see show_gate). Omit runId to follow the newest run. Set newPanel to open it in another panel instead.",
            inputSchema: {
                type: "object",
                additionalProperties: false,
                required: ["page"],
                properties: {
                    page: pageProperty,
                    runId: runIdProperty,
                    testId: { type: "string", description: "Trace only: test id (or `hooks:<suite id>`); defaults to the first failed test." },
                    attempt: { type: "integer", minimum: 0, description: "Trace only: zero-based attempt; defaults to the last one." },
                    newPanel: { type: "boolean", description: "Open the page in a new panel and leave this one as it is." },
                },
            },
            handler: async (panel, input) => {
                const path = formatRoute(input.page, { run: input.runId, test: input.testId, attempt: input.attempt });
                if (input.newPanel) return openPanel(panel, path);
                panel.navigate(path);
                return currentView(panel);
            },
        },
        {
            name: "get_view",
            description: "What a page shows: its route, view settings (`ui`), and what they resolve to. Defaults to the page on screen.",
            readOnly: true,
            inputSchema: { type: "object", additionalProperties: false, properties: { page: pageProperty } },
            handler: (panel, input) => currentView(panel, input.page),
        },
        {
            name: "set_view",
            description: `Change a page's view settings exactly as the user would; does not navigate. Fields by page: ${viewFieldsHelp(features)}.`,
            inputSchema: {
                type: "object",
                additionalProperties: false,
                required: ["page", "view"],
                properties: { page: pageProperty, view: { type: "object", description: "Fields to change." } },
            },
            handler: async (panel, input) => {
                const { feature, state, view } = panel.page(input.page);
                await feature.sync?.(state);
                view.set(input.view);
                return currentView(panel, input.page);
            },
        },
    ];
}