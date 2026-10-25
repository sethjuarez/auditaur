// Auditaur-enabled app sessions on this machine, as `auditaur apps --json` reports them.

import { cliJson } from "./cli.mjs";

// Readable telemetry stores, newest heartbeat first; sessions sharing one store collapse into its newest.
export async function listAppStores(state) {
    const { value, error } = await cliJson(state, ["apps"]);
    if (!Array.isArray(value)) return { stores: [], error: error ?? "`auditaur apps` returned no list" };
    const stores = new Map();
    for (const app of value) {
        if (!app.databaseReadable || !app.schemaValid || typeof app.databasePath !== "string") continue;
        const known = stores.get(app.databasePath);
        if (known) {
            known.sessions += 1;
            continue;
        }
        stores.set(app.databasePath, {
            db: app.databasePath,
            sessionId: app.sessionId,
            serviceName: app.serviceName,
            status: app.status,
            startedAt: app.startedAt,
            sessions: 1,
        });
    }
    return { stores: [...stores.values()], error: null };
}
