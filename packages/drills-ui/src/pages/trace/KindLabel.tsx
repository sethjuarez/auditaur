// A telemetry record's kind in its role color, e.g. "error" in the danger color.
import { kindOf, type TraceEvent } from "./types";

export function KindLabel({ event }: { event: TraceEvent }) {
  return <span className={`kind k-${kindOf(event)}`}>{event.kind}</span>;
}
