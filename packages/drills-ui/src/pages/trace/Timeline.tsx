// The attempt on one time axis: a lane of steps (click one to focus it) above a lane of telemetry
// ticks. Pointer-only; keyboard and screen-reader users pick steps in the step list below it.
import { ms } from "../../lib/format";
import { statusOf } from "../../lib/status";
import { kindOf, type TraceSelected } from "./types";

interface Props {
  selected: TraceSelected;
  kinds: string[];
  step: number | null;
  pick: (index: number) => void;
}

export function Timeline({ selected, kinds, step, pick }: Props) {
  const ends = [
    ...selected.steps.map((each) => (each.offsetMs ?? 0) + (each.durationMs ?? 0)),
    ...selected.events.map((event) => (event.offsetMs ?? 0) + (event.durationMs ?? 0)),
  ];
  const total = Math.max(1, selected.durationMs ?? 0, ...ends);
  const pct = (value: number | null) => Math.max(0, Math.min(100, (100 * (value ?? 0)) / total));
  const shown = selected.events.filter((event) => kinds.includes(kindOf(event)) && event.offsetMs != null);
  return (
    <div className="timeline" aria-hidden="true">
      <div className="lane">
        {selected.steps.map((each) => (
          <div
            key={each.index}
            className={`seg ${statusOf(each.status)}${step === each.index ? " sel" : ""}`}
            data-step={each.index}
            title={`Step ${each.index + 1}: ${each.title} · ${ms(each.durationMs, 2)}`}
            style={{ left: `${pct(each.offsetMs)}%`, width: `${Math.max(0.4, pct(each.durationMs))}%` }}
            onClick={() => pick(each.index)}
          />
        ))}
      </div>
      <div className="lane">
        {shown.map((event, index) => (
          <span key={index}>
            {event.durationMs ? (
              <span
                className={`tick span-bar k-${kindOf(event)}`}
                title={`${event.name} · ${ms(event.durationMs, 2)}`}
                style={{ left: `${pct(event.offsetMs)}%`, width: `${Math.max(0.3, pct(event.durationMs))}%` }}
              />
            ) : null}
            <span className={`tick k-${kindOf(event)}`} title={`${event.kind}: ${event.name}`} style={{ left: `${pct(event.offsetMs)}%` }} />
          </span>
        ))}
      </div>
      <div className="axis muted nums">
        <span>0</span>
        <span>{ms(total, 2)}</span>
      </div>
    </div>
  );
}
