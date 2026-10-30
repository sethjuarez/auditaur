// The drills canvas's action registry: the app's own actions (view-actions, panel-actions) and
// each page's domain actions. Page actions act on the panel's routed selection, as the page would.
// Any action not marked `readOnly` tells open pages to reload once it succeeds.

import { panelActions } from "./panel-actions.mjs";
import { viewActions } from "./view-actions.mjs";

// Page actions run against that page's state.
const pageAction = (feature) => (action) => ({
    ...action,
    handler: (panel, input) => action.handler(panel.page(feature.page).state, input, { navigate: (path) => panel.navigate(path) }),
});

// An action that takes no input says so, so the host rejects input it would otherwise ignore.
const NO_INPUT = { type: "object", additionalProperties: false, properties: {} };

// All actions, refusing duplicate names: one would silently shadow the other.
export function canvasActions(features, openPanel) {
    const actions = [
        ...viewActions(features, openPanel),
        ...panelActions(),
        ...features.flatMap((feature) => (feature.actions ?? []).map(pageAction(feature))),
    ].map((action) => (action.inputSchema ? action : { ...action, inputSchema: NO_INPUT }));
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