// Which code this provider runs, and whether the files on disk have changed since it started. A
// killed or edited extension keeps serving its old code until extensions reload, so the page and
// the `health` action say so instead of silently showing stale behavior.

import { createHash } from "node:crypto";
import { readdir, readFile } from "node:fs/promises";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { INSTALL } from "./paths.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const CODE = /\.(mjs|js|css)$/;
// The page bundle is read from disk on every request, so a new one is live without a reload.
const SERVED_FRESH = join(ROOT, "ui");

async function files(dir) {
    const entries = await readdir(dir, { withFileTypes: true });
    const nested = await Promise.all(entries.map((entry) => {
        const path = join(dir, entry.name);
        if (entry.isDirectory()) return path === SERVED_FRESH ? [] : files(path);
        return CODE.test(entry.name) ? [path] : [];
    }));
    return nested.flat();
}

// The provider's code files (relative, `/`-separated): what only an extension reload picks up.
export async function codeFiles() {
    return (await files(ROOT)).map((path) => relative(ROOT, path).replaceAll("\\", "/")).sort();
}

// A short hash of the provider's code files, in a stable order.
export async function fingerprint() {
    const hash = createHash("sha256");
    for (const name of await codeFiles()) {
        hash.update(name);
        hash.update(await readFile(join(ROOT, name)));
    }
    return hash.digest("hex").slice(0, 12);
}

const startedAt = new Date().toISOString();
const running = fingerprint();
const version = pluginVersion();
let failedPages = [];

// The Auditaur plugin version when this copy ships in the plugin
// (`<plugin>/com.github.copilot/extensions/auditaur-drills`); null for a committed copy.
export async function pluginVersion(root = ROOT) {
    try {
        const manifest = JSON.parse(await readFile(join(root, "..", "..", "..", "plugin.json"), "utf8"));
        return manifest.name === "auditaur" ? manifest.version ?? null : null;
    } catch {
        return null;
    }
}

// Pages whose feature module failed to load (see features.mjs).
export function reportFailedPages(failed) {
    failedPages = failed;
}

export async function health() {
    const build = await running;
    const onDisk = await fingerprint().catch(() => build);
    return {
        build,
        onDisk,
        stale: onDisk !== build,
        install: INSTALL,
        version: await version,
        startedAt,
        pid: process.pid,
        failedPages,
    };
}
