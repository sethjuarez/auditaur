

export const TRACE_EVENT_LIMIT = 2000;

export const DETAIL_LIMIT = 2000;

// The kinds the trace view filters by; "span-event" records count as "span".
export const TELEMETRY_KINDS = ["ipc", "event", "log", "error", "span"];

// ISO timestamps carry up to 9 fractional digits; Date.parse keeps only 3, so add the rest back.
export function isoMs(value) {
    if (typeof value !== "string") return null;
    const fraction = /\.(\d+)/.exec(value);
    const parsed = Date.parse(fraction ? value.replace(fraction[0], "") : value);
    return Number.isNaN(parsed) ? null : parsed + (fraction ? Number(`0.${fraction[1]}`) * 1000 : 0);
}

// Report numbers are untrusted: the page renders them without escaping.
export const num = (value) => (Number.isFinite(value) ? value : null);

export const nanosMs = (value) => (value == null || Number.isNaN(Number(value)) ? null : Number(value) / 1e6);

export function clip(value) {
    if (value == null) return "";
    const text = typeof value === "string" ? value : JSON.stringify(value);
    return text.length > DETAIL_LIMIT ? `${text.slice(0, DETAIL_LIMIT)}…` : text;
}

// Telemetry flattened onto one time axis: IPC calls, Tauri events, other spans, logs and errors.
export function traceEvents(telemetry) {
    const events = [];
    for (const span of telemetry?.spans ?? []) {
        const attributes = span.attributes ?? {};
        const start = nanosMs(span.startTimeUnixNanos);
        const end = nanosMs(span.endTimeUnixNanos);
        const ipc = attributes["tauri.command"] != null || String(span.name).startsWith("tauri.invoke");
        const event = String(span.name).startsWith("tauri.event");
        events.push({
            at: start,
            durationMs: start != null && end != null ? Math.max(0, end - start) : null,
            kind: ipc ? "ipc" : event ? "event" : "span",
            name: ipc
                ? attributes["tauri.command"] ?? span.name
                : event
                  ? `${attributes["tauri.event.direction"] ?? ""} ${String(span.name).replace(/^tauri\.event /, "")}`.trim()
                  : span.name,
            status: span.statusCode ?? null,
            source: span.source ?? null,
            detail: clip(ipc ? attributes["tauri.command.args"] : event ? attributes["tauri.event.payload"] : attributes),
        });
    }
    for (const log of telemetry?.logs ?? []) {
        events.push({
            at: nanosMs(log.timestampUnixNanos),
            durationMs: null,
            kind: "log",
            name: log.severityText ?? "log",
            status: null,
            source: log.source ?? null,
            detail: clip(log.body),
        });
    }
    for (const error of telemetry?.frontendErrors ?? []) {
        events.push({
            at: nanosMs(error.timestampUnixNanos),
            durationMs: null,
            kind: "error",
            name: error.errorType ?? "error",
            status: "Error",
            source: "frontend",
            detail: clip([error.message, error.stack].filter(Boolean).join("\n")),
        });
    }
    for (const event of telemetry?.spanEvents ?? []) {
        events.push({
            at: nanosMs(event.timestampUnixNanos ?? event.timeUnixNanos),
            durationMs: null,
            kind: "span-event",
            name: event.name ?? "span event",
            status: null,
            source: event.source ?? null,
            detail: clip(event.attributes ?? event),
        });
    }
    return events.filter((event) => event.at != null).sort((a, b) => a.at - b.at).slice(0, TRACE_EVENT_LIMIT);
}
