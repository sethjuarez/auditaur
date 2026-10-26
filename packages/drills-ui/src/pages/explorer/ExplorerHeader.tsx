// The explorer's header: counts and open-in links, run actions, then the filter and the last run.
import { IssueReopenedIcon, PlayIcon, SearchIcon, StopIcon, SyncIcon } from "@primer/octicons-react";
import { Button, IconButton, TextInput } from "@primer/react";
import { OpenInLinks, runLinks } from "../../components/OpenIn";
import { PageHeader } from "../../components/PageHeader";
import { useDebouncedField } from "../../hooks/useDebouncedField";
import type { ViewPatch } from "../../lib/canvasState";
import { ms, plural } from "../../lib/format";
import { isRunning, type ExplorerRun, type ExplorerState } from "./types";

interface Props {
  state: ExplorerState | null;
  busy: boolean;
  run: (path: string, body?: unknown) => Promise<unknown>;
  refresh: () => void;
  change: (patch: ViewPatch) => void;
}

function LastRun({ run }: { run: ExplorerRun | undefined }) {
  if (!run) return <span id="last-run" className="muted last-run">No runs yet</span>;
  const tone = run.status === "passed" ? "passed" : run.status === "interrupted" ? "interrupted" : "failed";
  const totals = run.totals ?? {};
  return (
    <span id="last-run" className="muted last-run nums">
      Last run <span className={`status-text ${tone}`}>{run.status}</span>
      {` · ${Number(totals.passed)}/${Number(totals.tests)} passed`}
      {totals.flaky ? ` · ${Number(totals.flaky)} flaky` : ""}
      {` · ${ms(run.durationMs)} · `}
      <span className="path">{run.runId}</span>
    </span>
  );
}

function Filter({ value, change }: { value: string; change: (patch: ViewPatch) => void }) {
  const [draft, setDraft] = useDebouncedField(value, (filter) => change({ filter }));
  return (
    <TextInput
      id="filter"
      type="search"
      className="explorer-filter"
      leadingVisual={SearchIcon}
      placeholder="Filter by title or @tag"
      aria-label="Filter tests"
      value={draft}
      onChange={(event) => setDraft(event.target.value)}
    />
  );
}

export function ExplorerHeader({ state, busy, run, refresh, change }: Props) {
  const tests = state?.suites.flatMap((suite) => suite.tests) ?? [];
  const last = state?.runs.find((each) => each.status !== "running");
  const disabled = !state || isRunning(state) || busy;
  const rerun = last ? Number(last.totals?.failed ?? 0) + Number(last.totals?.notRun ?? 0) : 0;
  const actions = (
    <>
      {state?.active && (
        <Button id="cancel" variant="danger" size="small" leadingVisual={StopIcon} onClick={() => void run("/api/cancel")}>Cancel run</Button>
      )}
      {rerun > 0 && (
        <Button
          id="run-failed"
          size="small"
          leadingVisual={IssueReopenedIcon}
          count={rerun}
          title={`Rerun the ${plural(rerun, "test")} that failed or did not run last time`}
          disabled={disabled}
          onClick={() => void run("/run", { lastFailed: true })}
        >
          Rerun failed
        </Button>
      )}
      <Button
        id="run-all"
        variant="primary"
        size="small"
        leadingVisual={PlayIcon}
        count={tests.length || undefined}
        title={`Run all ${plural(tests.length, "test")}`}
        disabled={disabled || !tests.length}
        onClick={() => void run("/run", {})}
      >
        Run all
      </Button>
      <IconButton id="refresh" size="small" icon={SyncIcon} aria-label="Reload drill files and results" onClick={refresh} />
    </>
  );
  const meta = state && (
    <span id="counts" className="muted nums">
      {`${plural(state.suites.length, "suite")} · ${plural(tests.length, "test")}`}
    </span>
  );
  return (
    <PageHeader title="Drill tests" meta={meta} actions={actions}>
      <div className="row explorer-toolbar">
        <Filter value={state?.ui.filter ?? ""} change={change} />
      </div>
      {state && (
        <div className="row last-run-bar">
          <LastRun run={last} />
          <span className="spacer" />
          <span id="open-in"><OpenInLinks caption="Open last run in" links={runLinks(last?.runId, null)} /></span>
        </div>
      )}
    </PageHeader>
  );
}
