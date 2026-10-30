// The coverage header: the run picker and reload, the shown run with links to its other canvases,
// the coverage meters, which list to show, and the app telemetry to compare against.
import { SyncIcon } from "@primer/octicons-react";
import { IconButton } from "@primer/react";
import { LoadAlert } from "../../components/LoadAlert";
import { OpenInLinks, runLinks } from "../../components/OpenIn";
import { PageHeader } from "../../components/PageHeader";
import { RunSelect } from "../../components/RunSelect";
import { StatusIcon } from "../../components/StatusIcon";
import { plural } from "../../lib/format";
import { CoverageMeters } from "./CoverageMeters";
import { CoverageTabs } from "./CoverageTabs";
import { MissedToggle } from "./MissedToggle";
import { StorePicker } from "./StorePicker";
import { baselineFor, type CoverageState, type CoverageTab } from "./types";

interface Props {
  state: CoverageState | null;
  loadError: string | null;
  busy: boolean;
  select: (runId: string | null) => void;
  reload: () => void;
  setTab: (tab: CoverageTab) => void;
  setMissedOnly: (missedOnly: boolean) => void;
  setStores: (dbs: string[]) => void;
}

export function CoverageHeader({ state, loadError, busy, select, reload, setTab, setMissedOnly, setStores }: Props) {
  const run = state?.run ?? null;
  const coverage = state?.coverage ?? null;
  const actions = (
    <>
      <RunSelect runs={state?.runs ?? null} pinned={state?.pinned ?? null} label="Run to measure" className="coverage-run" disabled={busy} pick={select} />
      <IconButton id="refresh" size="small" icon={SyncIcon} aria-label="Re-read coverage from the run and stores" onClick={reload} />
    </>
  );
  return (
    <PageHeader title="Drill coverage" meta={loadError && !state ? <span className="coverage-status"><LoadAlert /></span> : null} actions={actions}>
      {run && (
        <div className="row coverage-summary-row">
          <span className="coverage-summary muted nums">
            <StatusIcon status={run.status} label={`Run ${run.status}`} />
            <span id="summary">{run.runId}{coverage ? ` · ${plural(coverage.totals.tests, "test")}` : ""}</span>
          </span>
          <span className="spacer" />
          <span id="open-in"><OpenInLinks caption="Open this run in" links={runLinks(run.runId, "coverage")} /></span>
        </div>
      )}
      {coverage && <CoverageMeters totals={coverage.totals} baseline={{ commands: baselineFor(coverage, "commands"), events: baselineFor(coverage, "events") }} />}
      {coverage && state && (
        <div className="row coverage-toolbar">
          <CoverageTabs coverage={coverage} tab={state.ui.tab} missedOnly={state.ui.missedOnly} pick={setTab} />
          <MissedToggle checked={state.ui.missedOnly} set={setMissedOnly} />
        </div>
      )}
      {state && <StorePicker dbs={state.dbs} apps={run?.apps ?? []} busy={busy} apply={setStores} />}
    </PageHeader>
  );
}
