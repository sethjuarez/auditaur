// The run's status beside the title: a progress ring while it runs, a clock while it waits on
// someone, an alert when the page can't load, else the final result.
import { LoadAlert } from "../../components/LoadAlert";
import { ProgressRing } from "../../components/ProgressRing";
import { StatusIcon } from "../../components/StatusIcon";
import { STATUS_LABELS, statusOf } from "../../lib/status";
import { isDone, testsOf, type LiveState } from "./types";

export function RunStatus({ state, loadError }: { state: LiveState | null; loadError: string | null }) {
  const run = state?.run;
  if (!run) {
    return (
      <span id="status" className="run-status">
        {loadError && !state && <LoadAlert />}
      </span>
    );
  }
  if (run.status === "running") {
    const tests = testsOf(run);
    const done = tests.filter(isDone).length;
    const waiting = state.gates.length > 0;
    return (
      <span id="status" className="run-status">
        {waiting ? <StatusIcon status="waiting" /> : <ProgressRing value={done} max={tests.length} label={`${done} of ${tests.length} tests done`} />}
        <span className={`status-text ${waiting ? "waiting" : "running"}`}>{waiting ? "running · waiting for you" : "running"}</span>
      </span>
    );
  }
  const status = statusOf(run.status);
  return (
    <span id="status" className="run-status">
      <StatusIcon status={status} />
      <span className={`status-text ${status}`}>{STATUS_LABELS[status] === "not run" ? run.status : STATUS_LABELS[status]}</span>
    </span>
  );
}