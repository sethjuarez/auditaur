// The run's suites, or why there is nothing to show.
import { HistoryIcon } from "@primer/octicons-react";
import { Spinner } from "@primer/react";
import { EmptyState } from "../../components/EmptyState";
import { SuiteSection } from "./SuiteSection";
import type { LiveState } from "./types";

interface Props {
  state: LiveState | null;
  loadError: string | null;
  toggle: (id: string, open: boolean) => void;
}

export function RunTree({ state, loadError, toggle }: Props) {
  if (!state) {
    return <div id="tree">{!loadError && <div className="loading"><Spinner size="medium" srText="Loading" /></div>}</div>;
  }
  const run = state.run;
  if (!run) {
    return (
      <div id="tree">
        {state.pinned
          ? <EmptyState icon={HistoryIcon} title={<>Run <span className="path">{state.pinned}</span> was not found.</>}>Pick another run, or follow the latest.</EmptyState>
          : <EmptyState icon={HistoryIcon} title="No drill runs yet">Runs appear here from <span className="path">.auditaur/drill-runs</span> as soon as one starts.</EmptyState>}
      </div>
    );
  }
  const waitingTests = new Set(state.gates.map((gate) => gate.test));
  return (
    <div id="tree" className="run-tree">
      {run.suites.map((suite) => (
        <SuiteSection key={suite.id} suite={suite} runId={run.runId} expanded={state.ui.expanded} waitingTests={waitingTests} toggle={toggle} />
      ))}
    </div>
  );
}