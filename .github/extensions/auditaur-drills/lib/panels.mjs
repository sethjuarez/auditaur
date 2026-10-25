// Open panels by instance id. A panel is one canvas definition's state plus its server.

import { DrillError } from "./errors.mjs";
import { startServer, stopServer } from "./server.mjs";

export class Panels {
    #open = new Map();

    // Idempotent: reopening keeps the panel, and only non-empty input reconfigures it.
    async open(canvas, instanceId, input) {
        let panel = this.#open.get(instanceId);
        if (!panel) {
            const state = canvas.create(input);
            panel = { state, ...(await startServer(canvas.routes(state), canvas.page, canvas.components)) };
            this.#open.set(instanceId, panel);
        } else if (input && Object.keys(input).length > 0) {
            canvas.reconfigure(panel.state, input);
        }
        return panel;
    }

    state(instanceId) {
        const panel = this.#open.get(instanceId);
        if (!panel) throw new DrillError("not_open", "this canvas instance is not open");
        return panel.state;
    }

    async close(instanceId) {
        const panel = this.#open.get(instanceId);
        if (!panel) return;
        this.#open.delete(instanceId);
        await stopServer(panel.server);
    }
}
