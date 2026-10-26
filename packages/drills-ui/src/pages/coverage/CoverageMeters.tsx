// How much of the app the run exercised: commands, events, and tests whose telemetry was readable.
import { Meter } from "../../components/Meter";
import type { Coverage } from "./types";

export function CoverageMeters({ totals }: { totals: Coverage["totals"] }) {
  return (
    <div id="meters" className="row coverage-meters">
      <Meter label="Commands exercised" value={totals.commandsExercised} max={totals.commands} unit="commands exercised" />
      <Meter label="Events exercised" value={totals.eventsExercised} max={totals.events} unit="events exercised" />
      <Meter label="Tests with telemetry" value={totals.testsWithTelemetry} max={totals.tests} unit="tests with telemetry" />
    </div>
  );
}
