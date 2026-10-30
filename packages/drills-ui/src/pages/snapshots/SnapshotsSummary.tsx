// The shown run in one line: its id, how many snapshot mismatches it has, how many need review,
// and how many screenshots it saved.
import { StatusIcon } from "../../components/StatusIcon";
import { plural } from "../../lib/format";
import { isAcceptable, type SnapshotItem, type SnapshotsState } from "./types";

interface Props {
  run: NonNullable<SnapshotsState["run"]>;
  items: SnapshotItem[];
  screenshots: number;
}

export function SnapshotsSummary({ run, items, screenshots }: Props) {
  const pending = items.filter(isAcceptable).length;
  const mismatches = `${items.length} ${items.length === 1 ? "mismatch" : "mismatches"}`;
  const parts = [run.runId, mismatches, `${pending} to review`];
  if (screenshots) parts.push(plural(screenshots, "screenshot"));
  return (
    <span className="snapshots-summary muted nums">
      <StatusIcon status={run.status} label={`Run ${run.status}`} />
      <span id="summary">{parts.join(" · ")}</span>
    </span>
  );
}
