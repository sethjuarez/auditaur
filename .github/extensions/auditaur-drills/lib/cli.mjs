import { execFile } from "node:child_process";
import { readdir, stat } from "node:fs/promises";
import { join } from "node:path";
import { DRILLS_DIR } from "./paths.mjs";

export const LIST_TIMEOUT_MS = 60_000;

export function cliJson(state, args) {
    return new Promise((resolvePromise) => {
        execFile(
            state.cli,
            [...args, "--json"],
            { cwd: state.workspace, timeout: LIST_TIMEOUT_MS, windowsHide: true, maxBuffer: 32 * 1024 * 1024 },
            (error, stdout, stderr) => {
                try {
                    resolvePromise({ value: JSON.parse(stdout) });
                } catch {
                    const detail = (stderr || stdout || error?.message || "no output").trim();
                    resolvePromise({ error: `\`${state.cli} ${args.join(" ")}\` failed: ${detail.slice(-2000)}` });
                }
            },
        );
    });
}

export async function walkSignature(dir) {
    let signature = "";
    let entries;
    try {
        entries = await readdir(dir, { withFileTypes: true });
    } catch {
        return "missing";
    }
    for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
        const path = join(dir, entry.name);
        if (entry.isDirectory()) {
            if (entry.name !== "__snapshots__") {
                signature += `${entry.name}/{${await walkSignature(path)}}`;
            }
        } else if (entry.name.endsWith(".json")) {
            const info = await stat(path).catch(() => null);
            signature += `${entry.name}:${info?.mtimeMs}:${info?.size};`;
        }
    }
    return signature;
}

// `drill list` is re-run only when a drill file changes or a refresh is forced.
export async function loadSuites(state, force = false) {
    const signature = await walkSignature(join(state.workspace, DRILLS_DIR));
    if (!force && state.list && state.listSignature === signature) {
        return;
    }
    const { value, error } = await cliJson(state, ["drill", "list"]);
    state.listSignature = signature;
    state.list = value ?? null;
    state.listError = value ? null : error;
}
