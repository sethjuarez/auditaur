import { readdir, unlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

export const LOG_PREFIX = "auditaur-drills-canvas-";

export function processAlive(pid) {
    try {
        process.kill(pid, 0);
        return true;
    } catch (error) {
        return error.code === "EPERM";
    }
}

// Logs are deleted when a launched run exits, unless the extension that launched it died first.
export async function sweepOrphanLogs() {
    let names;
    try {
        names = await readdir(tmpdir());
    } catch {
        return;
    }
    for (const name of names) {
        const owner = Number(name.match(/^auditaur-drills-canvas-(\d+)-\d+\.log$/)?.[1]);
        if (owner && owner !== process.pid && !processAlive(owner)) {
            // A run still writing holds the file open on Windows; it is retried next startup.
            await unlink(join(tmpdir(), name)).catch(() => {});
        }
    }
}
