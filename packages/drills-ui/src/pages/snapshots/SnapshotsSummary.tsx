// The shown run in one line: its id, how many snapshot results it has, and how many need review.
import { StatusIcon } from "../../components/StatusIcon";
import { plural } from "../../lib/format";
import { isAcceptable, type SnapshotItem, type SnapshotsState } from "./types";

export function SnapshotsSummary({ run, items }: { run: NonNullable<SnapshotsState["run"]>; items: SnapshotItem[] }) {
  const pending = items.filter(isAcceptable).length;
  return (
    <span className="snapshots-summary muted nums">
      <StatusIcon status={run.status} label={`Run ${run.status}`} />
      <span id="summary">{`${run.runId} · ${plural(items.length, "snapshot result")} · ${pending} to review`}</span>
    </span>
  );
}
