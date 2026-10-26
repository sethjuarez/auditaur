// Toggles for which telemetry kinds the timeline and table show, each with its count.
import { Button } from "@primer/react";
import { KIND_LABELS, KINDS, kindOf, type TraceEvent } from "./types";

interface Props {
  kinds: string[];
  events: TraceEvent[];
  toggle: (kind: string) => void;
}

export function KindFilter({ kinds, events, toggle }: Props) {
  return (
    <div className="kind-filter" role="group" aria-label="Telemetry kinds to show">
      {KINDS.map((kind) => {
        const on = kinds.includes(kind);
        const count = events.filter((event) => kindOf(event) === kind).length;
        return (
          <Button
            key={kind}
            size="small"
            variant="invisible"
            className={`kind-toggle k-${kind}`}
            data-kind={kind}
            aria-pressed={on}
            title={`${on ? "Hide" : "Show"} ${KIND_LABELS[kind].noun} on the timeline and in the telemetry table`}
            leadingVisual={() => <span className="kind-dot" aria-hidden="true" />}
            count={count}
            onClick={() => toggle(kind)}
          >
            {KIND_LABELS[kind].label}
          </Button>
        );
      })}
    </div>
  );
}
