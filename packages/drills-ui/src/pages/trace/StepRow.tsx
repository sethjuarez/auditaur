// One step in the step list: status, start offset, phase and title, attachments, telemetry counts
// per kind, and duration. Rendered as a listbox option; the list handles keys and clicks.
import { PaperclipIcon } from "@primer/octicons-react";
import { Label } from "@primer/react";
import type { Ref } from "react";
import { StatusIcon } from "../../components/StatusIcon";
import { ms, plural } from "../../lib/format";
import { KIND_LABELS, KINDS, kindOf, type TraceEvent, type TraceStep } from "./types";

interface Props {
  step: TraceStep;
  events: TraceEvent[];
  selected: boolean;
  focusable: boolean;
  ref?: Ref<HTMLDivElement>;
}

function KindCounts({ events }: { events: TraceEvent[] }) {
  return (
    <span className="trace-step-kinds">
      {KINDS.map((kind) => {
        const count = events.filter((event) => kindOf(event) === kind).length;
        return count ? (
          <span key={kind} className={`kind-count k-${kind}`} title={`${count} ${KIND_LABELS[kind].noun} during this step`}>
            <span className="kind-dot" aria-hidden="true" />
            {count}
            <span className="visually-hidden">{` ${KIND_LABELS[kind].noun}`}</span>
          </span>
        ) : null;
      })}
    </span>
  );
}

export function StepRow({ step, events, selected, focusable, ref }: Props) {
  const files = step.attachments.length;
  return (
    <div
      ref={ref}
      id={`step-${step.index}`}
      className={`trace-step${selected ? " sel" : ""}`}
      role="option"
      aria-selected={selected}
      tabIndex={focusable ? 0 : -1}
      data-step={step.index}
    >
      <StatusIcon status={step.status} />
      <span className="trace-step-at muted mono nums">{step.offsetMs != null && `+${ms(step.offsetMs, 2)}`}</span>
      <span className="trace-step-title">
        {step.phase && step.phase !== "test" && <span className="trace-step-phase muted">{step.phase}</span>}
        <span className="trace-step-name">{step.title}</span>
        {step.soft && <Label size="small" variant="secondary">soft</Label>}
        {files > 0 && (
          <span className="trace-step-files muted" title={plural(files, "attachment")}>
            <PaperclipIcon size={12} aria-hidden="true" />
            {files}
            <span className="visually-hidden">{files === 1 ? " attachment" : " attachments"}</span>
          </span>
        )}
      </span>
      <KindCounts events={events} />
      <span className="duration muted nums">{ms(step.durationMs, 2)}</span>
    </div>
  );
}
