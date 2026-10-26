// What a test did, attempt by attempt: each attempt's steps and its error, then a link to its trace.
import { OpenInLinks } from "../../components/OpenIn";
import { StatusIcon } from "../../components/StatusIcon";
import { STATUS_LABELS, statusOf } from "../../lib/status";
import { StepList } from "./StepList";
import type { LiveAttempt, LiveTest } from "./types";

function Attempt({ attempt, headed }: { attempt: LiveAttempt; headed: boolean }) {
  const status = statusOf(attempt.status);
  return (
    <div className="attempt">
      {headed && (
        <div className="attempt-head">
          <StatusIcon status={status} />
          <span>{`Attempt ${Number(attempt.attempt) + 1}`}</span>
          <span className={`status-text ${status}`}>{STATUS_LABELS[status] === "not run" ? attempt.status : STATUS_LABELS[status]}</span>
        </div>
      )}
      <StepList steps={attempt.steps} />
      {attempt.error && !attempt.steps.some((step) => step.error) && <pre className="detail-error">{attempt.error}</pre>}
    </div>
  );
}

export function AttemptDetail({ test, runId }: { test: LiveTest; runId: string }) {
  if (!test.attempts.length) return <div className="muted">Not started.</div>;
  return (
    <>
      {test.attempts.map((attempt) => <Attempt key={attempt.attempt} attempt={attempt} headed={test.attempts.length > 1} />)}
      <div className="row detail-links">
        <OpenInLinks caption="Open this test in" links={[{ to: "trace", runId, testId: test.id }]} />
      </div>
    </>
  );
}