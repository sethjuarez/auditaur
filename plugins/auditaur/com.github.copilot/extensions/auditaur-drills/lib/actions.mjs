// Every agent action of the drills canvas: the app's own (navigate, views, screenshot, health,
// cancel) and each page's domain actions. Page actions act on the panel's routed selection, as the
// page would. Any action not marked `readOnly` tells open pages to reload once it succeeds.

import { captureScreenshot } from "./browser.mjs";
import { health } from "./build-info.mjs";
import { cancelRun } from "./launcher.mjs";
import { PAGES, formatRoute } from "./route.mjs";
import { runIdProperty } from "./schemas.mjs";

const pageProperty = { type: "string", enum: Object.keys(PAGES) };

// What a page shows: its view settings (`ui`) and what they resolve to. `describe` runs first
// because resolving may rescope the view (see ViewState.scope).
export async function currentView(panel, page = panel.route.page) {
    const { feature, state, view } = panel.page(page);
    const described = feature.describe ? await feature.describe(state) : {};
    return { page, route: panel.routeOf(page), ui: view.get(), ...described };
}

function viewFieldsHelp(features) {
    return features.map((feature) => `${feature.page}: ${Object.keys(feature.view ?? {}).join(", ") || "none"}`).join("; ");
}

function appActions(features, openPanel) {
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
        {
            name: "screenshot",
            description: "Capture the panel as the user sees it (PNG), with its current view.",
            readOnly: true,
            inputSchema: {
                type: "object",
                additionalProperties: false,
                properties: {
                    width: { type: "integer", minimum: 320, maximum: 3840, description: "Viewport width in pixels (default 1280)." },
                    height: { type: "integer", minimum: 200, maximum: 4000, description: "Viewport height in pixels (default 800)." },
                },
            },
            handler: async (panel, input, host) => {
                const png = await captureScreenshot(host.url, input);
                return {
                    textResultForLlm: JSON.stringify(await currentView(panel)),
                    binaryResultsForLlm: [{ type: "image", mimeType: "image/png", data: png.toString("base64"), description: "Auditaur drills panel" }],
                    resultType: "success",
                };
            },
        },
        {
            name: "health",
            description: "Which build the canvas runs, whether its files changed on disk since (then extensions need a reload), its install and plugin version, the CLI version and any mismatch, and the panel's route.",
            readOnly: true,
            handler: async (panel) => ({ ...(await health(panel.shared.cli)), route: panel.route }),
        },
        {
            name: "cancel",
            description: "Stop a drill run in progress; it finishes as interrupted. Defaults to the run this panel launched, then the newest running one.",
            inputSchema: { type: "object", additionalProperties: false, properties: { runId: runIdProperty } },
            handler: (panel, input) => cancelRun(panel.shared, input.runId ?? null),
        },
    ];
}

// Page actions run against that page's state.
const pageAction = (feature) => (action) => ({
    ...action,
    handler: (panel, input) => action.handler(panel.page(feature.page).state, input, { navigate: (path) => panel.navigate(path) }),
});

// All actions, refusing duplicate names: one would silently shadow the other.
export function canvasActions(features, openPanel) {
    const actions = [...appActions(features, openPanel), ...features.flatMap((feature) => (feature.actions ?? []).map(pageAction(feature)))];
    const seen = new Set();
    for (const { name } of actions) {
        if (seen.has(name)) throw new Error(`duplicate drills canvas action \`${name}\``);
        seen.add(name);
    }
    return actions;
}

// Runs an action, then tells open pages to reload unless it only reads.
export async function runAction(action, panel, input, host) {
    const value = await action.handler(panel, input ?? {}, host);
    if (!action.readOnly) panel.changed();
    return value;
}
