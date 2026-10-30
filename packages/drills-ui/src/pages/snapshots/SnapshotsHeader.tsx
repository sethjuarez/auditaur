// The snapshots header: the run picker and reload, a summary of the shown run with links to its
// other canvases, then the compare mode and "accept all" toolbar.
import { SyncIcon } from "@primer/octicons-react";
import { IconButton } from "@primer/react";
import { CompareModeControl, type CompareMode } from "../../components/ImageCompare";
import { LoadAlert } from "../../components/LoadAlert";
import { OpenInLinks, runLinks } from "../../components/OpenIn";
import { PageHeader } from "../../components/PageHeader";
import { RunSelect } from "../../components/RunSelect";
import { AcceptAllButton } from "./AcceptAllButton";
import { SnapshotsSummary } from "./SnapshotsSummary";
import { isAcceptable, type SnapshotsState } from "./types";

interface Props {
  state: SnapshotsState | null;
  loadError: string | null;
  busy: boolean;
  locked: boolean;
  select: (runId: string | null) => void;
  reload: () => void;
  setMode: (mode: CompareMode) => void;
  acceptAll: (ids: string[]) => void;
}

export function SnapshotsHeader({ state, loadError, busy, locked, select, reload, setMode, acceptAll }: Props) {
  const run = state?.run ?? null;
  const items = state?.items ?? [];
  const pending = items.filter(isAcceptable);
  // The mode only matters when some screenshot has both images to compare.
  const comparable = items.some((item) => item.kind === "screenshot" && item.files.expected && item.files.actual);
  const actions = (
    <>
      <RunSelect runs={state?.runs ?? null} pinned={state?.pinned ?? null} label="Run to review" className="snapshots-run" disabled={busy} pick={select} />
      <IconButton id="refresh" size="small" icon={SyncIcon} aria-label="Reload snapshots" onClick={reload} />
    </>
  );
  return (
    <PageHeader title="Drill snapshots" meta={loadError && !state ? <span className="snapshots-status"><LoadAlert /></span> : null} actions={actions}>
      {run && state && (
        <>
          <div className="row snapshots-summary-row">
            <SnapshotsSummary run={run} items={items} screenshots={state?.screenshots?.length ?? 0} />
            <span className="spacer" />
            <span id="open-in"><OpenInLinks caption="Open this run in" links={runLinks(run.runId, "snapshots")} /></span>
          </div>
          {(comparable || pending.length > 0) && (
            <div className="row snapshots-toolbar">
              {comparable && <CompareModeControl mode={state.ui.mode} pick={setMode} />}
              <span className="spacer" />
              {pending.length > 0 && <AcceptAllButton items={pending} running={run.status === "running"} locked={locked} accept={acceptAll} />}
            </div>
          )}
        </>
      )}
    </PageHeader>
  );
}
