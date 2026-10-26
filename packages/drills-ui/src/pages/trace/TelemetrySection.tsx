// The telemetry section: records in the shown window that pass the kind filter, then any notes the
// collector left about what it filtered out.
import { CounterLabel } from "@primer/react";
import { kindOf, type TraceSelected } from "./types";
import { TelemetryTable } from "./TelemetryTable";
import "../../components/Box.css";

function notesOf(notes: unknown): string | null {
  if (notes == null) return null;
  if (typeof notes === "string") return notes || null;
  if (typeof notes === "object" && Object.keys(notes).length) return JSON.stringify(notes);
  return null;
}

export function TelemetrySection({ selected, kinds, step }: { selected: TraceSelected; kinds: string[]; step: number | null }) {
  const inWindow = selected.events.filter((event) => step == null || event.step === step);
  const shown = inWindow.filter((event) => kinds.includes(kindOf(event)));
  const notes = notesOf(selected.filterNotes);
  return (
    <section className="box trace-section" aria-labelledby="telemetry-title">
      <div className="box-header">
        <h2 id="telemetry-title" className="box-title section-title">{step == null ? "Telemetry" : `Telemetry during step ${step + 1}`}</h2>
        <CounterLabel>{shown.length}</CounterLabel>
      </div>
      <TelemetryTable events={shown} hidden={inWindow.length - shown.length} />
      {notes && <div className="filter-notes muted">{notes}</div>}
    </section>
  );
}
