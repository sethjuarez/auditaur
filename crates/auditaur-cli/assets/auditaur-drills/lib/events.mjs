import { readFile, stat } from "node:fs/promises";
import { errorText } from "./runs.mjs";

export const EVENT_CACHE_LIMIT = 16;

export const eventCache = new Map();

export const scopeKey = (event) => `${event.suiteId}|${event.testId ?? ""}|${event.attempt ?? ""}`;

// Steps and pending gates come from the event log; `run.json` only carries test-level state.
export function foldEvents(text) {
    const steps = new Map();
    const gates = new Map();
    for (const line of text.split("\n")) {
        let event;
        try {
            event = JSON.parse(line);
        } catch {
            continue; // Blank, or the tail of a line still being written.
        }
        const key = scopeKey(event);
        if (event.type === "stepStarted") {
            if (!steps.has(key)) steps.set(key, []);
            steps.get(key).push({
                index: event.index,
                phase: event.phase,
                title: event.title,
                action: event.action,
                status: "running",
                soft: false,
                startedAt: event.ts,
                durationMs: null,
                error: null,
            });
        } else if (event.type === "stepFinished") {
            const step = (steps.get(key) ?? []).findLast((candidate) => candidate.index === event.index && candidate.status === "running");
            if (step) {
                Object.assign(step, { status: event.status, soft: event.soft, durationMs: event.durationMs, error: errorText(event.error) });
            }
        } else if (event.type === "gateWaiting") {
            gates.set(`${key}|${event.step}`, { ...event, since: event.ts });
        } else if (event.type === "gateResolved") {
            gates.delete(`${key}|${event.step}`);
        }
    }
    return { steps, gates: [...gates.values()] };
}

export async function readEvents(path) {
    const info = await stat(path).catch(() => null);
    if (!info) return { steps: new Map(), gates: [] };
    const cached = eventCache.get(path);
    if (cached && cached.size === info.size && cached.mtimeMs === info.mtimeMs) return cached.model;
    const text = await readFile(path, "utf8").catch(() => null);
    if (text == null) return cached?.model ?? { steps: new Map(), gates: [] };
    const model = foldEvents(text);
    eventCache.delete(path);
    eventCache.set(path, { size: info.size, mtimeMs: info.mtimeMs, model });
    if (eventCache.size > EVENT_CACHE_LIMIT) eventCache.delete(eventCache.keys().next().value);
    return model;
}
