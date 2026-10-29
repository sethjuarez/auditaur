// What the gate page shows: the gate whose request file the panel was pointed at, re-read on every
// load so it never shows an answer form for a gate that stopped waiting.

import { exists } from "../../lib/files.mjs";
import { isExpired, queuePaths, readRequest } from "../../lib/gate-queue.mjs";

const isoFromNanos = (nanos) => (nanos != null && Number.isFinite(Number(nanos)) ? new Date(Number(nanos) / 1e6).toISOString() : null);

// The published request in the shape the run page's gates use, so both render with one form.
function gateOf(request) {
    const prompt = request.prompt ?? {};
    return {
        requestId: request.requestId,
        test: "auditaur drill",
        attempt: request.attempt ?? 0,
        since: isoFromNanos(request.createdAtUnixNanos),
        name: prompt.name ?? request.gateId ?? "Manual gate",
        instructions: prompt.instructions ?? "",
        selector: prompt.selector ?? null,
        expectText: prompt.expectText ?? null,
        timeoutMs: prompt.timeoutMs ?? null,
        manualContinue: prompt.manualContinue ?? false,
        choices: Array.isArray(request.choices) ? request.choices : [],
        inputs: Array.isArray(request.inputs) ? request.inputs : [],
        clipboard: request.clipboard ?? null,
    };
}

// `status`: none (no gate given), pending, answered (the drill has not picked the answer up yet),
// expired, or closed (the request is gone: answered, timed out, or the drill stopped).
export async function gateView(state) {
    if (!state.gate) return { status: "none", requestId: null, requestPath: null, gate: null, session: null };
    const { requestId, requestPath, responsePath } = queuePaths(state.gate);
    const request = await readRequest(requestPath);
    const base = { requestId, requestPath };
    if (request?.requestId !== requestId) return { ...base, status: "closed", gate: null, session: null };
    const status = (await exists(responsePath)) ? "answered" : isExpired(request) ? "expired" : "pending";
    return {
        ...base,
        status,
        gate: gateOf(request),
        session: request.session ?? null,
        expiresAt: isoFromNanos(request.expiresAtUnixNanos),
    };
}

export const gateSummary = ({ status, requestId, gate }) => ({ status, requestId, name: gate?.name ?? null });
