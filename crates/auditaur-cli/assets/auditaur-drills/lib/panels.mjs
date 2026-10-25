// Open panels by instance id. A panel is one canvas definition's state plus its server. The state
// carries the panel's `view` (see view-state.mjs), which the canvas's own code may read.

import { DrillError } from "./errors.mjs";
import { startServer, stopServer } from "./server.mjs";
import { ViewState } from "./view-state.mjs";
import { withViewRoutes } from "./view-routes.mjs";

export class Panels {
    #open = new Map();

    // Idempotent: reopening keeps the panel, and only non-empty input reconfigures it.
    async open(canvas, instanceId, input) {
        let panel = this.#open.get(instanceId);
        if (!panel) {
            const view = new ViewState(canvas.view ?? {});
            const state = canvas.create(input, view);
            const routes = withViewRoutes(canvas.routes(state), view, async () => canvas.sync?.(state));
            panel = { state, ...(await startServer(routes, canvas.page, canvas.components)) };
            this.#open.set(instanceId, panel);
        } else if (input && Object.keys(input).length > 0) {
            canvas.reconfigure(panel.state, input);
            panel.state.view.changed();
        }
        return panel;
    }

    state(instanceId) {
        return this.#panel(instanceId).state;
    }

    // What actions may need beyond state: the panel's page `url`.
    panel(instanceId) {
        const { url } = this.#panel(instanceId);
        return { url };
    }

    #panel(instanceId) {
        const panel = this.#open.get(instanceId);
        if (!panel) throw new DrillError("not_open", "this canvas instance is not open");
        return panel;
    }

    async close(instanceId) {
        const panel = this.#open.get(instanceId);
        if (!panel) return;
        this.#open.delete(instanceId);
        await stopServer(panel.server);
    }
}
