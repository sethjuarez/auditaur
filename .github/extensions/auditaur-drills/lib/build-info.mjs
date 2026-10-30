// Which code this provider runs, and whether the files on disk have changed since it started. A
// killed or edited extension keeps serving its old code until extensions reload, so the page and
// the `health` action say so instead of silently showing stale behavior.

import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { readdir, readFile } from "node:fs/promises";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { INSTALL } from "./paths.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
// Code, plus the version stamps: a new stamp on disk is an upgrade this process hasn't loaded.
const CODE = /\.(mjs|js|css)$|^(version|installed)\.json$/;
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
const version = shippedVersion();
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

async function stampVersion(path) {
    try {
        const stamp = JSON.parse(await readFile(path, "utf8"));
        return typeof stamp.version === "string" ? stamp.version : null;
    } catch {
        return null;
    }
}

// The CLI version `auditaur init extension` stamped into `installed.json`; null when absent.
export const installedVersion = (root = ROOT) => stampVersion(join(root, "installed.json"));

// The release stamped into `version.json` when it shipped; every copy carries it.
export const bundledVersion = (root = ROOT) => stampVersion(join(root, "version.json"));

// Which release this copy's code came from, and how it got here: `plugin`, `init` (the
// `auditaur init extension` stamp), or `bundled` (any other copy, such as one installed from a
// release tag). Null only when the copy carries no stamp at all.
export async function shippedVersion(root = ROOT) {
    const plugin = await pluginVersion(root);
    if (plugin) return { version: plugin, source: "plugin" };
    const installed = await installedVersion(root);
    if (installed) return { version: installed, source: "init" };
    const bundled = await bundledVersion(root);
    return bundled ? { version: bundled, source: "bundled" } : { version: null, source: null };
}

// Pages whose feature module failed to load (see features.mjs).
export function reportFailedPages(failed) {
    failedPages = failed;
}

const VERSION_TTL_MS = 60_000;
const cliVersions = new Map();

function runVersion(cli) {
    return new Promise((done) => {
        execFile(cli, ["--version"], { timeout: 10_000, windowsHide: true }, (error, stdout) => {
            const version = /\d+\.\d+\.\d+\S*/.exec(stdout ?? "")?.[0] ?? null;
            done(version ? { version, error: null } : { version: null, error: (error?.message ?? "no version").trim() });
        });
    });
}

// `<cli> --version`, cached briefly so the half-minute health poll doesn't spawn a process each time.
export function cliVersion(cli, run = runVersion, now = Date.now()) {
    const cached = cliVersions.get(cli);
    if (cached && now - cached.at < VERSION_TTL_MS) return cached.value;
    const value = run(cli);
    cliVersions.set(cli, { at: now, value });
    return value;
}

// The canvas and the CLI it drives ship together; a different CLI may not read what the canvas
// expects. `source` says which fix applies. Null when either version is unknown.
export function versionSkew(shipped, cli) {
    const canvas = shipped?.version;
    return canvas && cli && canvas !== cli ? { canvas, cli, source: shipped.source } : null;
}

export async function health(cli) {
    const build = await running;
    const onDisk = await fingerprint().catch(() => build);
    // What this process loaded at start; a newer copy on disk shows as `stale` until a reload.
    const shipped = await version;
    const onDiskVersion = (await shippedVersion().catch(() => shipped)).version;
    const cliInfo = cli ? await cliVersion(cli) : { version: null, error: null };
    return {
        build,
        onDisk,
        stale: onDisk !== build,
        install: INSTALL,
        version: shipped.version,
        versionSource: shipped.source,
        onDiskVersion,
        cli: { path: cli ?? null, ...cliInfo },
        skew: versionSkew(shipped, cliInfo.version),
        startedAt,
        pid: process.pid,
        failedPages,
    };
}
