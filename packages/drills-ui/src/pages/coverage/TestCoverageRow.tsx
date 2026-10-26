// One test: its result, the commands and events it touched, and whether its telemetry was readable.
import { Label } from "@primer/react";
import { StatusIcon } from "../../components/StatusIcon";
import { plural } from "../../lib/format";
import type { TestCoverage } from "./types";

const NOTES = {
  recorded: () => null,
  none: () => "No telemetry recorded.",
  unreadable: (error?: string) => `Telemetry unreadable: ${error ?? ""}`,
  partial: (error?: string) => `Some attempts' telemetry is unreadable: ${error ?? ""}`,
};

export function TestCoverageRow({ test }: { test: TestCoverage }) {
  const touched = [...test.commands, ...test.events];
  const note = (NOTES[test.telemetry] ?? NOTES.none)(test.error);
  const readable = test.telemetry === "recorded" || test.telemetry === "partial";
  return (
    <li className="box-row coverage-item">
      <StatusIcon status={test.status} />
      <div className="coverage-item-main">
        <div className="coverage-item-title">{test.title}</div>
        {readable && (
          <div className="coverage-item-tests">
            {touched.length
              ? touched.map((name) => <Label key={name} size="small" variant="secondary" className="tag mono">{name}</Label>)
              : <span className="muted">No IPC or events.</span>}
          </div>
        )}
        {note && <div className="coverage-item-note muted">{note}</div>}
      </div>
      <div className="coverage-item-stats muted nums">
        <span>{plural(test.commands.length, "command")}</span>
        <span>{plural(test.events.length, "event")}</span>
      </div>
    </li>
  );
}
