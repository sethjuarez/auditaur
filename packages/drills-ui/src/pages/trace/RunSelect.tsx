// Which run the trace shows: the newest, or one picked run.
import { RunSelect as BaseRunSelect } from "../../components/RunSelect";
import type { TraceState } from "./types";

interface Props {
  state: TraceState | null;
  disabled: boolean;
  pick: (runId: string | null) => void;
}

export function RunSelect({ state, disabled, pick }: Props) {
  return <BaseRunSelect runs={state?.runs ?? null} pinned={state?.pinned ?? null} label="Run to trace" className="trace-run" disabled={disabled} pick={pick} />;
}
