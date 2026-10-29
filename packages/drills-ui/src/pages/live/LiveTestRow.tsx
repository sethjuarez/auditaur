// One test of the run: status (a clock while it waits on a gate), name, labels, and time; opens to
// its attempts.
import { Label } from "@primer/react";
import { DisclosureRow } from "../../components/DisclosureRow";
import { StatusIcon } from "../../components/StatusIcon";
import { Tags } from "../../components/Tags";
import { ms, since } from "../../lib/format";
import { AttemptDetail } from "./AttemptDetail";
import type { LiveTest } from "./types";

interface Props {
  test: LiveTest;
  runId: string;
  waiting: boolean;
  open: boolean;
  toggle: () => void;
}

export function LiveTestRow({ test, runId, waiting, open, toggle }: Props) {
  const last = test.attempts.at(-1);
  const live = last?.status === "running" ? since(last.steps[0]?.startedAt) : null;
  const status = waiting && test.result === "running" ? "waiting" : test.result;
  const name = (
    <>
      <span className="test-title">{test.name}</span>
      <Tags tags={test.tags} />
      {test.attempts.length > 1 && <Label size="small" variant="attention">{`${test.attempts.length} attempts`}</Label>}
    </>
  );
  return (
    <DisclosureRow
      id={test.id}
      open={open}
      toggle={toggle}
      status={<StatusIcon status={status} />}
      name={name}
      trailing={<span className="duration muted nums">{ms(live ?? test.durationMs)}</span>}
    >
      <AttemptDetail test={test} runId={runId} />
    </DisclosureRow>
  );
}