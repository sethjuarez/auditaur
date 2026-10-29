// Which run a page shows: the newest ("Latest run"), or one picked run.
import { Select } from "@primer/react";

interface Props {
  runs: { runId: string; status: string }[] | null;
  pinned: string | null;
  label: string;
  disabled: boolean;
  pick: (runId: string | null) => void;
  className?: string;
}

export function RunSelect({ runs, pinned, label, disabled, pick, className }: Props) {
  return (
    <Select
      id="run"
      size="small"
      aria-label={label}
      className={className}
      value={pinned ?? ""}
      disabled={disabled || !runs}
      onChange={(event) => pick(event.target.value || null)}
    >
      <Select.Option value="">Latest run</Select.Option>
      {runs?.map((run) => (
        <Select.Option key={run.runId} value={run.runId}>{`${run.runId} · ${run.status}`}</Select.Option>
      ))}
    </Select>
  );
}
