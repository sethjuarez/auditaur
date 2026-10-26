// Which run the trace shows: the newest, or one picked run.
import { Select } from "@primer/react";
import type { TraceState } from "./types";

interface Props {
  state: TraceState | null;
  disabled: boolean;
  pick: (runId: string | null) => void;
}

export function RunSelect({ state, disabled, pick }: Props) {
  return (
    <Select
      id="run"
      size="small"
      aria-label="Run to trace"
      className="trace-run"
      value={state?.pinned ?? ""}
      disabled={disabled || !state}
      onChange={(event) => pick(event.target.value || null)}
    >
      <Select.Option value="">Latest run</Select.Option>
      {state?.runs.map((run) => (
        <Select.Option key={run.runId} value={run.runId}>{`${run.runId} · ${run.status}`}</Select.Option>
      ))}
    </Select>
  );
}
