// Which code this provider runs, and whether the files on disk have changed since it started. A
// killed or edited extension keeps serving its old code until extensions reload, so the page and
// the `health` action say so instead of silently showing stale behavior.

import { createHash } from "node:crypto";
import { readdir, readFile } from "node:fs/promises";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const CODE = /\.(mjs|js|css)$/;

async function files(dir) {
    const entries = await readdir(dir, { withFileTypes: true });
    const nested = await Promise.all(entries.map((entry) => {
        const path = join(dir, entry.name);
        if (entry.isDirectory()) return files(path);
        return CODE.test(entry.name) ? [path] : [];
    }));
    return nested.flat();
}

// A short hash of every code file, in a stable order.
export async function fingerprint() {
    const hash = createHash("sha256");
    for (const path of (await files(ROOT)).sort()) {
        hash.update(relative(ROOT, path).replaceAll("\\", "/"));
        hash.update(await readFile(path));
    }
    return hash.digest("hex").slice(0, 12);
}

const startedAt = new Date().toISOString();
const running = fingerprint();
let failedPages = [];

// Pages whose feature module failed to load (see features.mjs).
export function reportFailedPages(failed) {
    failedPages = failed;
}

export async function health() {
    const build = await running;
    const onDisk = await fingerprint().catch(() => build);
    return { build, onDisk, stale: onDisk !== build, startedAt, pid: process.pid, failedPages };
}
