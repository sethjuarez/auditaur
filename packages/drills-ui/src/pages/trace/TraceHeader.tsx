// The trace header: the run picker and reload, then the test and attempt pickers, then a summary of
// the traced attempt and links to the run's other canvases.
import { SyncIcon } from "@primer/octicons-react";
import { IconButton } from "@primer/react";
import { LoadAlert } from "../../components/LoadAlert";
import { OpenInLinks, runLinks } from "../../components/OpenIn";
import { PageHeader } from "../../components/PageHeader";
import { AttemptSelect } from "./AttemptSelect";
import { RunSelect } from "./RunSelect";
import { TestSelect } from "./TestSelect";
import { TraceSummary } from "./TraceSummary";
import type { SelectBody, TraceState } from "./types";

interface Props {
  state: TraceState | null;
  loadError: string | null;
  busy: boolean;
  select: (body: SelectBody) => void;
  reload: () => void;
}

export function TraceHeader({ state, loadError, busy, select, reload }: Props) {
  const selected = state?.selected ?? null;
  const pinned = state?.pinned ?? null;
  const entry = state?.entries.find((each) => each.key === selected?.key);
  const actions = (
    <>
      <RunSelect state={state} disabled={busy} pick={(runId) => select({ runId })} />
      <IconButton id="refresh" size="small" icon={SyncIcon} aria-label="Reload the trace" onClick={reload} />
    </>
  );
  return (
    <PageHeader title="Drill trace" meta={loadError && !state ? <span className="trace-status"><LoadAlert /></span> : null} actions={actions}>
      {state?.run && (
        <>
          <div className="row trace-pickers">
            <TestSelect entries={state.entries} value={selected?.key ?? ""} disabled={busy} pick={(testId) => select({ runId: pinned, testId })} />
            {entry && selected && (
              <AttemptSelect
                attempts={entry.attempts}
                value={selected.attempt}
                disabled={busy}
                pick={(attempt) => select({ runId: pinned, testId: selected.key, attempt })}
              />
            )}
          </div>
          <div className="row trace-summary-row">
            <TraceSummary selected={selected} />
            <span className="spacer" />
            <span id="open-in"><OpenInLinks caption="Open this run in" links={runLinks(state.run.runId, "trace")} /></span>
          </div>
        </>
      )}
    </PageHeader>
  );
}
