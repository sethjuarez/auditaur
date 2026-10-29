// The traced attempt in one line: status, time, steps, and telemetry records.
import { StatusIcon } from "../../components/StatusIcon";
import { ms, plural } from "../../lib/format";
import { STATUS_LABELS, statusOf } from "../../lib/status";
import type { TraceSelected } from "./types";

export function TraceSummary({ selected }: { selected: TraceSelected | null }) {
  if (!selected) return <span id="summary" className="trace-summary muted" />;
  const status = statusOf(selected.status);
  const parts = [ms(selected.durationMs, 2), plural(selected.steps.length, "step"), plural(selected.events.length, "telemetry record")].filter(Boolean);
  return (
    <span id="summary" className="trace-summary muted nums">
      <StatusIcon status={status} />
      <span><span className={`status-text ${status}`}>{STATUS_LABELS[status]}</span>{parts.map((part) => ` · ${part}`).join("")}</span>
    </span>
  );
}
