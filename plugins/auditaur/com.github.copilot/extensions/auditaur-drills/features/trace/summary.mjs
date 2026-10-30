// The trace view as the `get_trace` agent action reports it: each step with the telemetry it caused.

export function traceSummary(view) {
    const selected = view.selected;
    return {
        runId: view.run?.runId ?? null,
        status: view.run?.status ?? null,
        tests: view.entries.map(({ key, title, result, attempts }) => ({ testId: key, title, result, attempts: attempts.length })),
        selected: selected && {
            testId: selected.key,
            title: selected.title,
            attempt: selected.attempt,
            status: selected.status,
            error: selected.error,
            steps: selected.steps.map((step) => ({
                index: step.index,
                title: step.title,
                status: step.status,
                offsetMs: step.offsetMs,
                durationMs: step.durationMs,
                error: step.error,
                attachments: step.attachments.map((attachment) => attachment.path),
                telemetry: selected.events
                    .filter((event) => event.step === step.index)
                    .map(({ kind, name, status, offsetMs, detail }) => ({ kind, name, status, offsetMs, detail: detail.slice(0, 200) })),
            })),
            beforeFirstStep: selected.events.filter((event) => event.step == null).length,
        },
    };
}