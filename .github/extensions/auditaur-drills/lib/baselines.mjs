// Snapshot baselines: where a run said one lives, and replacing it with a run's actual output.

import { lstat, mkdir, readFile, realpath, rename, rm, writeFile } from "node:fs/promises";
import { basename, dirname, isAbsolute, join, relative, resolve } from "node:path";
import { randomUUID } from "node:crypto";
import { DrillError } from "./errors.mjs";

// `<dir>/__snapshots__/<suite>/<name>.aria.yml` or `<name>-<os>.png`, as the runner names them.
const BASELINE = /(^|\/)__snapshots__\/[^/]+\/[^/]+(\.aria\.yml|-[a-z0-9]+\.png)$/;

const outside = (root, path) => {
    const inside = relative(root, path);
    return !inside || inside.startsWith("..") || isAbsolute(inside);
};

// The baseline file a run reported (relative to the workspace), or an error if it is anything else.
export function baselinePath(workspace, display) {
    const text = String(display ?? "").replace(/\\/g, "/");
    if (!BASELINE.test(text) || text.split("/").includes("..")) {
        throw new DrillError("not_baseline", "not a snapshot baseline path");
    }
    const full = resolve(workspace, text);
    if (outside(resolve(workspace), full)) throw new DrillError("not_baseline", "the baseline is outside the workspace");
    return full;
}

export const baselineKind = (display) => (String(display).endsWith(".png") ? "screenshot" : "aria");

// The baseline's bytes, or null when there is none yet. Other read errors are thrown.
export async function readBaseline(full) {
    return readFile(full).catch((error) => {
        if (error.code === "ENOENT") return null;
        throw error;
    });
}

// The real path of `path`'s nearest existing ancestor (or itself).
async function nearestReal(path) {
    for (let current = path; ; current = dirname(current)) {
        const real = await realpath(current).catch((error) => {
            if (error.code === "ENOENT") return null;
            throw error;
        });
        if (real) return real;
        if (dirname(current) === current) throw new DrillError("not_baseline", "the baseline folder does not exist");
    }
}

async function assertInside(realWorkspace, path) {
    const real = await nearestReal(path);
    if (real !== realWorkspace && outside(realWorkspace, real)) {
        throw new DrillError("not_baseline", "the baseline folder links outside the workspace");
    }
    return real;
}

// Writes `bytes` as the baseline atomically. A folder that links outside the workspace is refused
// before anything is created, and the temp file is unique so concurrent writes cannot mix.
export async function writeBaseline(workspace, full, bytes) {
    const dir = dirname(full);
    const realWorkspace = await realpath(workspace);
    await assertInside(realWorkspace, dir);
    await mkdir(dir, { recursive: true });
    const realDir = await assertInside(realWorkspace, dir);
    const target = join(realDir, basename(full));
    const existing = await lstat(target).catch(() => null);
    if (existing && !existing.isFile()) throw new DrillError("not_baseline", "the baseline is not a regular file");
    const temp = join(realDir, `.${basename(full)}.${randomUUID()}.tmp`);
    try {
        await writeFile(temp, bytes, { flag: "wx" });
        await rename(temp, target);
    } catch (error) {
        await rm(temp, { force: true });
        throw error;
    }
}