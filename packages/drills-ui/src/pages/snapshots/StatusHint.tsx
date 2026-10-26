// Why a result has no accept button: what its status means and what to do instead.
import { InfoIcon } from "@primer/octicons-react";
import { statusInfo, type SnapshotItem } from "./types";

export function StatusHint({ item }: { item: SnapshotItem }) {
  const hint = statusInfo(item).hint;
  if (!hint) return null;
  return (
    <p className="snap-hint muted">
      <InfoIcon size={16} />
      <span>{hint}</span>
    </p>
  );
}
