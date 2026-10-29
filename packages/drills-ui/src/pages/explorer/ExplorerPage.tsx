// Explorer: every drill suite and test with its latest result and history; run, rerun, or cancel.
import { BeakerIcon, SearchIcon } from "@primer/octicons-react";
import { Spinner } from "@primer/react";
import { EmptyState } from "../../components/EmptyState";
import { useAction } from "../../hooks/useAction";
import { useCanvasState } from "../../hooks/useCanvasState";
import { ExplorerHeader } from "./ExplorerHeader";
import { ExplorerNotices } from "./ExplorerNotices";
import { SuiteCard } from "./SuiteCard";
import { isRunning, type ExplorerState } from "./types";
import "./explorer.css";

export function ExplorerPage() {
  const { state, error, reload, change } = useCanvasState<ExplorerState>((current) => (isRunning(current) ? 1000 : 5000));
  const action = useAction(reload);

  if (!state) {
    return (
      <>
        <ExplorerHeader state={null} busy run={action.run} refresh={() => void reload("?refresh")} change={change} />
        <main id="main" className="page-main">
          {error ? <ExplorerNotices state={null} actionError={null} loadError={error} /> : <div className="loading"><Spinner size="medium" srText="Loading" /></div>}
        </main>
      </>
    );
  }

  const disabled = isRunning(state) || action.busy;
  const toggle = (id: string) => {
    const expanded = state.ui.expanded.includes(id) ? state.ui.expanded.filter((each) => each !== id) : [...state.ui.expanded, id];
    change({ expanded });
  };
  const suites = state.suites.filter((suite) => suite.tests.some((test) => test.shown));

  return (
    <>
      <ExplorerHeader state={state} busy={action.busy} run={action.run} refresh={() => void reload("?refresh")} change={change} />
      <main id="main" className="page-main">
        <ExplorerNotices state={state} actionError={action.error} loadError={error} />
        {suites.map((suite) => (
          <SuiteCard key={suite.path} suite={suite} expanded={state.ui.expanded} disabled={disabled} run={action.run} toggle={toggle} />
        ))}
        {!suites.length && !state.listError && (state.suites.length
          ? <EmptyState icon={SearchIcon} title="No tests match the filter." />
          : <EmptyState icon={BeakerIcon} title="No drill tests yet">Add suites under <span className="path">.auditaur/drills</span>.</EmptyState>)}
      </main>
    </>
  );
}
