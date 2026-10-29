// The page body: why there is nothing to show, or the selected coverage list or map.
import { HistoryIcon, InfoIcon, CheckCircleIcon } from "@primer/octicons-react";
import { Spinner } from "@primer/react";
import { EmptyState } from "../../components/EmptyState";
import { CommandList } from "./CommandList";
import { CoverageMap } from "./CoverageMap";
import { EventList } from "./EventList";
import { TestCoverageRow } from "./TestCoverageRow";
import { shownCommands, shownEvents, shownTests, type Coverage, type CoverageState } from "./types";

const EMPTY = {
  commands: "No IPC commands were recorded. Compare with app telemetry to list commands no test called.",
  events: "No events were recorded. Compare with app telemetry to list events no test touched.",
  tests: "No tests in this run.",
  map: "Nothing to map.",
};

function CoverageList({ coverage, ui, comparing }: { coverage: Coverage; ui: CoverageState["ui"]; comparing: boolean }) {
  const titles = new Map(coverage.tests.map((test) => [test.id, test.title]));
  const empty = ui.missedOnly
    ? <EmptyState icon={CheckCircleIcon} title="Nothing unexercised here." />
    : <EmptyState icon={InfoIcon} title={EMPTY[ui.tab]} />;
  if (ui.tab === "commands") {
    const items = shownCommands(coverage, ui.missedOnly);
    return items.length ? <CommandList items={items} titles={titles} comparing={comparing} /> : empty;
  }
  if (ui.tab === "events") {
    const items = shownEvents(coverage, ui.missedOnly);
    return items.length ? <EventList items={items} titles={titles} comparing={comparing} /> : empty;
  }
  if (ui.tab === "tests") {
    const tests = shownTests(coverage, ui.missedOnly);
    return tests.length
      ? <ul className="box coverage-list" aria-label="Tests">{tests.map((test) => <TestCoverageRow key={test.id} test={test} />)}</ul>
      : empty;
  }
  const mapped = coverage.tests.length && (ui.missedOnly
    ? shownCommands(coverage, true).length + shownEvents(coverage, true).length
    : coverage.commands.length + coverage.events.length);
  return mapped ? <CoverageMap coverage={coverage} missedOnly={ui.missedOnly} /> : empty;
}

export function CoverageBody({ state, loadError }: { state: CoverageState | null; loadError: string | null }) {
  if (!state) return loadError ? null : <div className="loading"><Spinner size="medium" srText="Loading" /></div>;
  if (!state.run) {
    return state.pinned
      ? <EmptyState icon={HistoryIcon} title={<>Run <span className="path">{state.pinned}</span> was not found.</>}>Pick another run, or go back to the latest.</EmptyState>
      : <EmptyState icon={HistoryIcon} title="No drill runs yet">Runs appear here from <span className="path">.auditaur/drill-runs</span> once a drill runs.</EmptyState>;
  }
  if (!state.coverage) return <EmptyState icon={InfoIcon} title="Coverage is unavailable for this run." />;
  return <CoverageList coverage={state.coverage} ui={state.ui} comparing={state.dbs.length > 0} />;
}
