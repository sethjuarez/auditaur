// Which attempt of a retried test to trace; only shown when there is more than one.
import { Select } from "@primer/react";
import { STATUS_LABELS, statusOf } from "../../lib/status";
import type { TraceEntry } from "./types";

interface Props {
  attempts: TraceEntry["attempts"];
  value: number | null;
  disabled: boolean;
  pick: (attempt: number) => void;
}

export function AttemptSelect({ attempts, value, disabled, pick }: Props) {
  const valid = attempts.filter((each) => Number.isSafeInteger(each.attempt) && each.attempt >= 0);
  if (valid.length < 2) return null;
  return (
    <Select
      id="attempt"
      size="small"
      aria-label="Attempt to trace"
      className="trace-attempt"
      value={String(value ?? "")}
      disabled={disabled}
      onChange={(event) => pick(Number(event.target.value))}
    >
      {valid.map((each) => (
        <Select.Option key={each.attempt} value={String(each.attempt)}>{`Attempt ${each.attempt + 1} · ${STATUS_LABELS[statusOf(each.status)]}`}</Select.Option>
      ))}
    </Select>
  );
}
