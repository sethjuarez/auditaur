// The agent side of a panel's view state: `get_view` and `set_view` for every canvas, and the rule
// that any action not marked `readOnly` notifies open pages once it succeeds, exactly like a page POST.

import { viewSchema } from "./view-state.mjs";

// `canvas.describe(state)` may add what the view resolves to (the step shown, the tests the filter
// leaves visible, ...), so the agent sees what the user sees without a second call. It runs first
// because resolving may rescope the view (see ViewState.scope).
async function currentView(canvas, state) {
    const described = canvas.describe ? await canvas.describe(state) : {};
    return { ui: state.view.get(), ...described };
}

export function viewActions(canvas) {
    const fields = canvas.view ?? {};
    return [
        {
            name: "get_view",
            description: "What the panel is showing: its view settings (`ui`) and what they resolve to.",
            readOnly: true,
            handler: (state) => currentView(canvas, state),
        },
        {
            name: "set_view",
            description: `Change what the panel shows, exactly as the user would. Fields: ${Object.keys(fields).join(", ") || "none"}.`,
            inputSchema: viewSchema(fields),
            handler: async (state, input) => {
                await canvas.sync?.(state);
                state.view.set(input);
                return currentView(canvas, state);
            },
        },
    ];
}

// Runs an action, then tells open pages to reload unless the action only reads.
export async function runAction(action, state, input) {
    const value = await action.handler(state, input);
    if (!action.readOnly) state.view.changed();
    return value;
}
