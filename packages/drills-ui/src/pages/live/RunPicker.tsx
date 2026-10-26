// Which run the page shows: the newest (following) or one picked run.
import { Select } from "@primer/react";
import type { LiveState } from "./types";

interface Props {
  state: LiveState | null;
  disabled: boolean;
  follow: (runId: string | null) => void;
}

export function RunPicker({ state, disabled, follow }: Props) {
  return (
    <Select
      id="pick"
      size="small"
      aria-label="Run to show"
      className="run-picker"
      value={state?.pinned ?? ""}
      disabled={disabled || !state}
      onChange={(event) => follow(event.target.value || null)}
    >
      <Select.Option value="">Follow latest</Select.Option>
      {state?.runs.map((run) => (
        <Select.Option key={run.runId} value={run.runId}>{`${run.runId} · ${run.status}`}</Select.Option>
      ))}
    </Select>
  );
}