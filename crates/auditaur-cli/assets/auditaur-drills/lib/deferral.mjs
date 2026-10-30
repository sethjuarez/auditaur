// An installed copy steps aside for a repository's committed copy. If that copy never registers
// (it failed to start, or the host didn't load it), nothing serves the canvas; say so instead of
// leaving the session without it. Canvases are declared only when an extension joins, so the
// installed copy can't take over.

export const DEFER_GRACE_MS = 20_000;
const POLL_MS = 1_000;

/**
 * Wait until some provider declares `canvasId`. Resolves `"registered"`, `"missing"` after
 * `graceMs`, or `"unknown"` when the host can't list canvases.
 */
export async function awaitCommittedCanvas({ list, canvasId, graceMs = DEFER_GRACE_MS, pollMs = POLL_MS, sleep = delay }) {
    if (typeof list !== "function") return "unknown";
    const deadline = Date.now() + graceMs;
    for (;;) {
        let canvases;
        try {
            canvases = (await withTimeout(list(), Math.max(deadline - Date.now(), pollMs)))?.canvases;
        } catch {
            return "unknown";
        }
        if (!Array.isArray(canvases)) return "unknown";
        if (canvases.some((canvas) => canvas?.canvasId === canvasId)) return "registered";
        if (Date.now() >= deadline) return "missing";
        await sleep(pollMs);
    }
}

export function missingCanvasMessage(committedPath, graceMs = DEFER_GRACE_MS) {
    return (
        `Auditaur Drills: this repository commits its own canvas (${committedPath}), so the installed copy stepped aside, ` +
        `but no auditaur-drills canvas had registered ${Math.round(graceMs / 1000)} s after this session started. If the canvas ` +
        "is still missing, the committed copy failed to start or wasn't loaded in this session. Check its extension log, update it with `auditaur init extension --force` " +
        "and start a new session (or reload extensions), or delete it to use the installed copy."
    );
}

function withTimeout(promise, ms) {
    let timer;
    const timeout = new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error("timed out")), ms);
    });
    return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));
}

function delay(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}
