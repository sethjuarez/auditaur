// What the shown step left behind: its error and attachments. With no step shown, the whole
// attempt: records from before the first step and the attempt's own attachments.
import { RowsIcon } from "@primer/octicons-react";
import { Button } from "@primer/react";
import { ErrorText } from "../../components/ErrorText";
import { StatusIcon } from "../../components/StatusIcon";
import { plural } from "../../lib/format";
import { AttachmentList } from "./AttachmentView";
import type { TraceSelected } from "./types";
import "../../components/Box.css";

interface Props {
  selected: TraceSelected;
  runId: string;
  step: number | null;
  showAll: () => void;
}

export function StepDetail({ selected, runId, step, showAll }: Props) {
  const whole = selected.kind === "hooks" ? "suite" : "attempt";
  const chosen = step == null ? null : selected.steps[step];
  if (!chosen) {
    const before = selected.events.filter((event) => event.step == null).length;
    return (
      <section className="box trace-section step-detail" aria-labelledby="step-detail-title">
        <div className="box-header">
          <h2 id="step-detail-title" className="box-title section-title">{`Whole ${whole}`}</h2>
        </div>
        <div className="section-body">
          {before > 0 && <div className="note muted">{`${plural(before, "record")} arrived before the first step.`}</div>}
          {selected.attachments.length > 0 && <AttachmentList attachments={selected.attachments} runId={runId} context={`for the whole ${whole}`} />}
          {selected.steps.length > 0
            ? <div className="note muted">Select a step to see its screenshots and the telemetry recorded during it.</div>
            : !selected.attachments.length && <div className="note muted">{`Nothing was attached to this ${whole}.`}</div>}
        </div>
      </section>
    );
  }
  const context = `for step ${chosen.index + 1}: ${chosen.title}`;
  return (
    <section className="box trace-section step-detail" aria-labelledby="step-detail-title">
      <div className="box-header">
        <StatusIcon status={chosen.status} />
        <h2 id="step-detail-title" className="box-title section-title">
          <span className="muted">{`Step ${chosen.index + 1}`}</span> {chosen.title}
        </h2>
        <span className="spacer" />
        <Button size="small" variant="invisible" leadingVisual={RowsIcon} data-step="all" title={`Stop focusing this step and show the whole ${whole}`} onClick={showAll}>
          {`Show whole ${whole}`}
        </Button>
      </div>
      <div className="section-body">
        {chosen.error && <ErrorText>{chosen.error}</ErrorText>}
        {chosen.attachments.length
          ? <AttachmentList attachments={chosen.attachments} runId={runId} context={context} />
          : <div className="note muted">No screenshots for this step. Run with <span className="path">--trace on</span> to capture every step.</div>}
      </div>
    </section>
  );
}
