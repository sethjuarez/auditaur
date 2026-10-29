// A test's or hook's steps: status, phase, title, and time, with any error beneath.
import { Label } from "@primer/react";
import { StatusIcon } from "../../components/StatusIcon";
import { ms, since } from "../../lib/format";
import type { LiveStep } from "./types";

function StepRow({ step }: { step: LiveStep }) {
  const duration = step.status === "running" ? since(step.startedAt) : step.durationMs;
  return (
    <li className="live-step">
      <StatusIcon status={step.status} />
      <span className="live-step-title">
        {step.phase && step.phase !== "test" && <span className="live-step-phase muted">{step.phase}</span>}
        <span>{step.title}</span>
        {step.soft && <Label size="small" variant="secondary">soft</Label>}
      </span>
      <span className="duration muted nums">{ms(duration)}</span>
      {step.error && <pre className="detail-error live-step-error">{step.error}</pre>}
    </li>
  );
}

export function StepList({ steps }: { steps: LiveStep[] }) {
  if (!steps.length) return <div className="muted">No steps yet.</div>;
  return <ol className="live-steps">{steps.map((step, index) => <StepRow key={index} step={step} />)}</ol>;
}