// The run's tests as one bar split by result, so failures show at a glance.
import { ProgressBar } from "@primer/react";
import { countResult, isDone, RESULTS, testsOf, type LiveRun } from "./types";

export function RunBar({ run }: { run: LiveRun }) {
  const tests = testsOf(run);
  if (!tests.length) return null;
  const done = tests.filter(isDone).length;
  return (
    <ProgressBar
      id="bar"
      className="run-bar"
      barSize="small"
      role="progressbar"
      aria-label="Tests done"
      aria-valuemin={0}
      aria-valuemax={tests.length}
      aria-valuenow={done}
      aria-valuetext={`${done} of ${tests.length} tests done`}
    >
      {RESULTS.filter((result) => countResult(tests, result) > 0).map((result) => (
        <ProgressBar.Item
          key={result}
          className={`run-bar-item ${result}`}
          progress={(100 * countResult(tests, result)) / tests.length}
          aria-hidden="true"
        />
      ))}
    </ProgressBar>
  );
}