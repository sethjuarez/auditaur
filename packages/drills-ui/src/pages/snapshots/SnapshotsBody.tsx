// The page body under the header: why there is nothing to review, or a card per snapshot result,
// then the run's saved screenshots.
import { CheckCircleIcon, HistoryIcon, InfoIcon } from "@primer/octicons-react";
import { Spinner } from "@primer/react";
import { EmptyState } from "../../components/EmptyState";
import { SavedScreenshots } from "./SavedScreenshots";
import { SnapshotCard, type SplitControl } from "./SnapshotCard";
import type { SnapshotsState } from "./types";

interface Props {
  state: SnapshotsState | null;
  loadError: string | null;
  locked: boolean;
  split: SplitControl;
  accept: (ids: string[]) => void;
}

export function SnapshotsBody({ state, loadError, locked, split, accept }: Props) {
  if (!state) return loadError ? null : <div className="loading"><Spinner size="medium" srText="Loading" /></div>;
  const run = state.run;
  if (!run) {
    return state.pinned
      ? <EmptyState icon={HistoryIcon} title={<>Run <span className="path">{state.pinned}</span> was not found.</>}>Pick another run, or go back to the latest.</EmptyState>
      : <EmptyState icon={HistoryIcon} title="No drill runs yet">Runs appear here from <span className="path">.auditaur/drill-runs</span> once a drill runs.</EmptyState>;
  }
  const shots = <SavedScreenshots shots={state.screenshots ?? []} runId={run.runId} />;
  if (!state.items.length) {
    return (
      <>
        <EmptyState icon={run.status === "passed" ? CheckCircleIcon : InfoIcon} title={noMismatchTitle(run.status)}>
          Snapshot steps that match their baseline are not listed here.
        </EmptyState>
        {shots}
      </>
    );
  }
  return (
    <div className="snaps">
      {state.items.map((item) => (
        <SnapshotCard key={item.id} item={item} runId={run.runId} running={run.status === "running"} locked={locked} mode={state.ui.mode} split={split} accept={accept} />
      ))}
      {shots}
    </div>
  );
}

// Only a finished, passing run can vouch for its snapshots; anything else may have skipped some.
function noMismatchTitle(status: string): string {
  if (status === "running") return "No snapshot mismatches so far.";
  if (status === "passed") return "No snapshot mismatches in this run.";
  return "No snapshot mismatches, but the run did not pass, so some snapshot steps may not have run.";
}
