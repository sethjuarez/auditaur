// Puts a loaded attempt's steps and telemetry on one time axis, measured from the attempt's start.

import { errorText } from "../../lib/runs.mjs";
import { isoMs, num, traceEvents } from "../../lib/telemetry.mjs";

const fileRef = ({ name, contentType, path }) => ({ name, contentType, path });

export function alignTimeline(detail) {
    const starts = detail.steps.map((step) => isoMs(step.startedAt));
    const origin = isoMs(detail.startedAt) ?? starts.find((start) => start != null) ?? null;
    const offset = (at) => (origin == null || at == null ? null : Math.round((at - origin) * 10) / 10);
    // Each event belongs to the latest step that started before it: effects often land just after
    // the step that caused them returns.
    const stepOf = (at) => {
        let found = null;
        starts.forEach((start, index) => {
            if (start != null && start <= at) found = index;
        });
        return found;
    };
    return {
        key: detail.key,
        kind: detail.kind,
        title: detail.title,
        attempt: num(detail.attempt),
        status: detail.status,
        durationMs: num(detail.durationMs),
        error: detail.error,
        missing: detail.missing ?? false,
        filterNotes: detail.telemetry?.filterNotes ?? null,
        steps: detail.steps.map((step, index) => ({
            index,
            phase: step.phase,
            title: step.title,
            action: step.action,
            status: step.status,
            soft: step.soft ?? false,
            offsetMs: offset(starts[index]),
            durationMs: num(step.durationMs),
            error: errorText(step.error),
            attachments: (step.attachments ?? []).map(fileRef),
        })),
        attachments: detail.attachments.filter((attachment) => attachment.name !== "telemetry").map(fileRef),
        events: traceEvents(detail.telemetry).map((event) => ({ ...event, offsetMs: offset(event.at), step: stepOf(event.at) })),
    };
}