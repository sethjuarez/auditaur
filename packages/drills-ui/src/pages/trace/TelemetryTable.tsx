// The telemetry recorded in the shown window (a step, or the whole attempt), oldest first: when,
// what kind, its name with status and duration, and its payload.
import { Label } from "@primer/react";
import { ms } from "../../lib/format";
import { KindLabel } from "./KindLabel";
import type { TraceEvent } from "./types";

function EventStatus({ status }: { status: string | null }) {
  if (!status || status === "Unset") return null;
  return <Label size="small" variant={/error/i.test(status) ? "danger" : "secondary"}>{status}</Label>;
}

function TelemetryRow({ event }: { event: TraceEvent }) {
  return (
    <tr className={`event-row k-row-${event.kind}`}>
      <td className="at muted mono nums">{event.offsetMs != null && `+${ms(event.offsetMs, 2)}`}</td>
      <td className="event-kind"><KindLabel event={event} /></td>
      <td className="event-name">
        <span>{event.name}</span>
        <EventStatus status={event.status} />
        {event.durationMs ? <span className="muted nums">{ms(event.durationMs, 2)}</span> : null}
      </td>
      <td className="detail mono">{event.detail}</td>
    </tr>
  );
}

interface Props {
  events: TraceEvent[];
  // Records in this window the kind filter hides.
  hidden: number;
}

export function TelemetryTable({ events, hidden }: Props) {
  if (!events.length) {
    return (
      <div className="telemetry-empty muted">
        {hidden ? `The kind filter hides all ${hidden} records here.` : "No telemetry in this window."}
      </div>
    );
  }
  return (
    <div className="telemetry-scroll">
      <table className="telemetry">
        <colgroup><col className="at" /><col className="event-kind" /><col className="event-name" /><col /></colgroup>
        <thead className="visually-hidden">
          <tr><th scope="col">Time</th><th scope="col">Kind</th><th scope="col">Name</th><th scope="col">Detail</th></tr>
        </thead>
        <tbody>{events.map((event, index) => <TelemetryRow key={index} event={event} />)}</tbody>
      </table>
    </div>
  );
}
