// The shown run in one line: id, progress, result counts, and elapsed or total time.
import { ms, since } from "../../lib/format";
import { countResult, isDone, RESULTS, testsOf, type LiveRun } from "./types";

export function RunSummary({ run }: { run: LiveRun }) {
  const tests = testsOf(run);
  const done = tests.filter(isDone).length;
  const elapsed = ms(run.status === "running" ? since(run.startedAt) : run.durationMs);
  return (
    <span id="summary" className="muted run-summary nums">
      <span className="path">{run.runId}</span>
      {` · ${done}/${tests.length} done`}
      {RESULTS.filter((result) => countResult(tests, result) > 0).map((result) => (
        <span key={result}>{" · "}<span className={`status-text ${result}`}>{`${countResult(tests, result)} ${result}`}</span></span>
      ))}
      {elapsed && ` · ${elapsed}`}
    </span>
  );
}