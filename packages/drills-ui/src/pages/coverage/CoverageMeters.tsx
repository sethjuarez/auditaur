// How much of the app the run exercised: commands, events, and tests whose telemetry was readable.
// Commands are measured against the registered commands or app telemetry, events only against
// app telemetry; without a baseline they are plain counts.
import { InfoIcon } from "@primer/octicons-react";
import { Meter } from "../../components/Meter";
import { TouchedCount } from "./TouchedCount";
import type { Coverage } from "./types";

interface Props {
  totals: Coverage["totals"];
  baseline: { commands: boolean; events: boolean };
}

export function CoverageMeters({ totals, baseline }: Props) {
  return (
    <>
      <div id="meters" className="row coverage-meters">
        {baseline.commands
          ? <Meter label="Commands exercised" value={totals.commandsExercised} max={totals.commands} unit="commands exercised" />
          : <TouchedCount label="Commands" value={totals.commandsExercised} unit="commands" />}
        {baseline.events
          ? <Meter label="Events exercised" value={totals.eventsExercised} max={totals.events} unit="events exercised" />
          : <TouchedCount label="Events" value={totals.eventsExercised} unit="events" />}
        <Meter label="Tests with telemetry" value={totals.testsWithTelemetry} max={totals.tests} unit="tests with telemetry" />
      </div>
      {!(baseline.commands && baseline.events) && (
        <p id="no-baseline" className="muted coverage-no-baseline">
          <InfoIcon size={14} /> {noBaseline(baseline)}
        </p>
      )}
    </>
  );
}

function noBaseline(baseline: Props["baseline"]) {
  if (baseline.commands) return "Events have no baseline: they count only what the run touched. Compare with app telemetry below to find events no test touched.";
  return "No baseline: these count only what the run touched, so nothing can show as missed. List the app's commands (src-tauri/src or coverage.commands in .auditaur/config.json), or compare with app telemetry below.";
}
