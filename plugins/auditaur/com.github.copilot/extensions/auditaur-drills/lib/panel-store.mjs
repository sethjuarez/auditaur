// Where a panel's route and view settings survive provider restarts: one JSON file per panel in the
// Copilot session's workspace, so a rehydrated panel lands where the user left it. Without a session
// workspace nothing is written and panels start fresh after a restart.

import { mkdir, readFile, rename, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";

const VERSION = 1;
// Matches the runtime's instance id rule, so an id can never leave the folder.
const INSTANCE_ID = /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/;

export class PanelStore {
    #dir = null;

    // `workspacePath` from the joined session; undefined keeps the store in memory only.
    bind(workspacePath) {
        this.#dir = workspacePath ? join(workspacePath, "auditaur-drills", "panels") : null;
    }

    #file(instanceId) {
        return this.#dir && INSTANCE_ID.test(instanceId) ? join(this.#dir, `${instanceId}.json`) : null;
    }

    // The saved `{ route, views, dbs }`, or null when there is none or it is unreadable or outdated.
    async load(instanceId) {
        const file = this.#file(instanceId);
        if (!file) return null;
        try {
            const saved = JSON.parse(await readFile(file, "utf8"));
            return saved?.version === VERSION && saved.data && typeof saved.data === "object" ? saved.data : null;
        } catch {
            return null;
        }
    }

    async save(instanceId, data) {
        const file = this.#file(instanceId);
        if (!file) return;
        await mkdir(this.#dir, { recursive: true });
        const temp = `${file}.${process.pid}.${Date.now()}.tmp`;
        await writeFile(temp, JSON.stringify({ version: VERSION, data }), "utf8");
        await rename(temp, file);
    }

    async remove(instanceId) {
        const file = this.#file(instanceId);
        if (file) await rm(file, { force: true });
    }
}
