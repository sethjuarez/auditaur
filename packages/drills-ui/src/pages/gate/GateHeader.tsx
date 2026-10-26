// The gate page header: the gate's name and whether it is still waiting.
import { Label } from "@primer/react";
import { PageHeader } from "../../components/PageHeader";
import type { GateState, GateStatus } from "./types";

const LABELS: Record<GateStatus, { text: string; variant: "attention" | "success" | "danger" | "secondary" }> = {
  none: { text: "No gate", variant: "secondary" },
  pending: { text: "Waiting for you", variant: "attention" },
  answered: { text: "Answered", variant: "success" },
  expired: { text: "Timed out", variant: "danger" },
  closed: { text: "Closed", variant: "secondary" },
};

export function GateHeader({ state }: { state: GateState | null }) {
  const label = state && LABELS[state.status];
  return (
    <PageHeader
      title={state?.gate?.name ?? "Manual gate"}
      meta={label && <Label id="gate-status" variant={label.variant}>{label.text}</Label>}
    />
  );
}
