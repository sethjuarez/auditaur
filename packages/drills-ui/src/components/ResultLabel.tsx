// Spells out results that a muted icon beside a duration would let pass for green: a skipped test may
// have run for a while before it stopped, so its time alone must not read as a pass.
import { Label } from "@primer/react";
import { statusOf } from "../lib/status";

export function ResultLabel({ result, reason }: { result: unknown; reason?: string | null }) {
  if (statusOf(result) !== "skipped") return null;
  return (
    <Label size="small" variant="secondary" className="result-label" title={reason ?? "Skipped"}>
      skipped
    </Label>
  );
}
