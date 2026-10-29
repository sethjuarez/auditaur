// The run page header: status, the run picker and cancel, then the summary, open-in links, and bar.
import { StopIcon } from "@primer/octicons-react";
import { Button } from "@primer/react";
import { OpenInLinks, runLinks } from "../../components/OpenIn";
import { PageHeader } from "../../components/PageHeader";
import { RunBar } from "./RunBar";
import { RunPicker } from "./RunPicker";
import { RunStatus } from "./RunStatus";
import { RunSummary } from "./RunSummary";
import type { LiveState } from "./types";

interface Props {
  state: LiveState | null;
  loadError: string | null;
  busy: boolean;
  follow: (runId: string | null) => void;
  cancel: () => void;
}

export function LiveHeader({ state, loadError, busy, follow, cancel }: Props) {
  const run = state?.run ?? null;
  const actions = (
    <>
      <RunPicker state={state} disabled={busy} follow={follow} />
      {run?.status === "running" && (
        <Button id="cancel" variant="danger" size="small" leadingVisual={StopIcon} title="Stop this run; it finishes as interrupted" disabled={busy} onClick={cancel}>
          Cancel run
        </Button>
      )}
    </>
  );
  return (
    <PageHeader title="Drill run" meta={<RunStatus state={state} loadError={loadError} />} actions={actions}>
      {run && (
        <>
          <div className="row run-summary-row">
            <RunSummary run={run} />
            <span className="spacer" />
            <span id="open-in"><OpenInLinks caption="Open this run in" links={runLinks(run.runId, "run")} /></span>
          </div>
          <RunBar run={run} />
        </>
      )}
    </PageHeader>
  );
}