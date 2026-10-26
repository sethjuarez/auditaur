// Which test (or suite's hooks) of the run to trace, grouped by suite, each with its result.
import { Select } from "@primer/react";
import { STATUS_LABELS, statusOf } from "../../lib/status";
import type { TraceEntry } from "./types";

interface Props {
  entries: TraceEntry[];
  value: string;
  disabled: boolean;
  pick: (testId: string) => void;
}

function bySuite(entries: TraceEntry[]) {
  const groups = new Map<string, TraceEntry[]>();
  for (const entry of entries) groups.set(entry.suite, [...(groups.get(entry.suite) ?? []), entry]);
  return [...groups];
}

export function TestSelect({ entries, value, disabled, pick }: Props) {
  return (
    <Select
      id="test"
      size="small"
      aria-label="Test to trace"
      className="trace-test"
      value={value}
      disabled={disabled || !entries.length}
      onChange={(event) => pick(event.target.value)}
    >
      {bySuite(entries).map(([suite, group]) => (
        <Select.OptGroup key={suite} label={suite}>
          {group.map((entry) => (
            <Select.Option key={entry.key} value={entry.key}>{`${entry.title} · ${STATUS_LABELS[statusOf(entry.result)]}`}</Select.Option>
          ))}
        </Select.OptGroup>
      ))}
    </Select>
  );
}
