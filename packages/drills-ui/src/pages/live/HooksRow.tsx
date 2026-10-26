// A suite's setup and teardown hooks as one row that opens to their steps.
import { DisclosureRow } from "../../components/DisclosureRow";
import { StatusIcon } from "../../components/StatusIcon";
import { StepList } from "./StepList";
import type { LiveStep } from "./types";

export const hooksStatus = (hooks: LiveStep[]) =>
  hooks.some((step) => step.status === "running") ? "running"
    : hooks.some((step) => step.status === "failed" || step.status === "timedOut") ? "failed"
      : "passed";

export function HooksRow({ id, hooks, open, toggle }: { id: string; hooks: LiveStep[]; open: boolean; toggle: () => void }) {
  return (
    <DisclosureRow id={id} open={open} toggle={toggle} status={<StatusIcon status={hooksStatus(hooks)} />} name={<span className="muted">Suite hooks</span>}>
      <StepList steps={hooks} />
    </DisclosureRow>
  );
}