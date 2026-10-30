// How much of the app the run exercised: commands, events, and tests whose telemetry was readable.
// Commands and events are only measured against app telemetry; without it they are plain counts.
import { InfoIcon } from "@primer/octicons-react";
import { Meter } from "../../components/Meter";
import { TouchedCount } from "./TouchedCount";
import type { Coverage } from "./types";

export function CoverageMeters({ totals, baseline }: { totals: Coverage["totals"]; baseline: boolean }) {
  return (
    <>
      <div id="meters" className="row coverage-meters">
        {baseline ? (
          <>
            <Meter label="Commands exercised" value={totals.commandsExercised} max={totals.commands} unit="commands exercised" />
            <Meter label="Events exercised" value={totals.eventsExercised} max={totals.events} unit="events exercised" />
          </>
        ) : (
          <>
            <TouchedCount label="Commands" value={totals.commandsExercised} unit="commands" />
            <TouchedCount label="Events" value={totals.eventsExercised} unit="events" />
          </>
        )}
        <Meter label="Tests with telemetry" value={totals.testsWithTelemetry} max={totals.tests} unit="tests with telemetry" />
      </div>
      {!baseline && (
        <p id="no-baseline" className="muted coverage-no-baseline">
          <InfoIcon size={14} /> No baseline: these count only what the run touched, so nothing can show as missed. Compare with app telemetry below to find commands and events no test exercised.
        </p>
      )}
    </>
  );
}
