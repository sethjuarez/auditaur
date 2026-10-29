// Open panels by instance id. Each is an AppPanel (route, views, shared state) served by its own
// loopback server. Panels save their route and views to the store, so a provider restart rehydrates
// them where the user left them.

import { randomUUID } from "node:crypto";
import { apiRoutes } from "./api-routes.mjs";
import { AppPanel } from "./app-panel.mjs";
import { inheritedInput } from "./config.mjs";
import { DrillError } from "./errors.mjs";
import { canonicalRoute } from "./route.mjs";
import { startServer, stopServer } from "./server.mjs";

export const CANVAS_ID = "auditaur-drills";
// How long a rehydrating open waits for the session workspace (where saved panels live) before
// starting without its saved route.
const STORE_WAIT_MS = 3000;

export class Panels {
    #features;
    #store;
    #open = new Map();
    #rpc = null;
    #storeReady;
    #markStoreReady;

    constructor({ features, store }) {
        this.#features = features;
        this.#store = store;
        this.#storeReady = new Promise((resolve) => (this.#markStoreReady = resolve));
    }

    // After joinSession: where panels are saved, and the RPC client that opens new panels.
    bind({ workspacePath, rpc }) {
        this.#store.bind(workspacePath);
        this.#rpc = rpc?.canvas?.open ? rpc : null;
        this.#markStoreReady();
    }

    // Idempotent: an open panel is returned as is. A rehydrate replays the original input, which must
    // not undo what the user did since, so input only configures a panel that is not open yet.
    open(instanceId, input) {
        let entry = this.#open.get(instanceId);
        if (!entry) {
            entry = this.#create(instanceId, input);
            this.#open.set(instanceId, entry);
            entry.catch(() => this.#open.delete(instanceId));
        }
        return entry;
    }

    async #create(instanceId, input) {
        await Promise.race([this.#storeReady, new Promise((resolve) => setTimeout(resolve, STORE_WAIT_MS).unref?.())]);
        const saved = await this.#store.load(instanceId);
        const panel = new AppPanel({
            features: this.#features,
            input,
            saved,
            save: (data) => this.#store.save(instanceId, data),
        });
        const { server, url } = await startServer(apiRoutes(panel, (from, route) => this.openNew(from, route)));
        return { panel, server, url };
    }

    async get(instanceId) {
        const entry = this.#open.get(instanceId);
        if (!entry) throw new DrillError("not_open", "this canvas instance is not open");
        return entry;
    }

    // Opens `route` in a new panel that inherits `from`'s configuration (not its route).
    async openNew(from, route) {
        const path = canonicalRoute(route);
        if (!this.#rpc) throw new DrillError("panels_unavailable", "this session cannot open canvases from a canvas");
        const instanceId = `${CANVAS_ID}-${randomUUID()}`;
        const input = { ...inheritedInput(from.input), dbs: from.shared.dbs, route: path };
        try {
            await this.#rpc.canvas.open({ canvasId: CANVAS_ID, instanceId, input });
        } catch (error) {
            throw new DrillError("open_failed", `could not open a new panel: ${error?.message ?? error}`);
        }
        return { instanceId, route: path };
    }

    async close(instanceId) {
        const entry = this.#open.get(instanceId);
        if (!entry) return;
        this.#open.delete(instanceId);
        const { panel, server } = await entry.catch(() => ({}));
        if (!panel) return;
        await Promise.all([panel.dispose().then(() => this.#store.remove(instanceId)).catch(() => {}), stopServer(server)]);
    }
}
