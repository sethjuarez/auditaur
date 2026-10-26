// What the gate page shows for each status: the form while the gate waits, otherwise why not.
import { PersonIcon } from "@primer/octicons-react";
import { EmptyState } from "../../components/EmptyState";
import { GateForm, type GateAnswer } from "../../components/gate";
import { Notice } from "../../components/Notice";
import { GateSession } from "./GateSession";
import type { GateState } from "./types";

interface Props {
  state: GateState;
  answer: (answer: GateAnswer) => Promise<boolean>;
}

function GateStatusBody({ state, answer }: Props) {
  switch (state.status) {
    case "pending":
      return state.gate ? <GateForm key={state.gate.requestId} gate={state.gate} answer={answer} /> : null;
    case "answered":
      return <Notice tone="ok" title="Answered">The drill continues once it picks the answer up.</Notice>;
    case "expired":
      return <Notice tone="warning" title="This gate timed out">The drill stopped waiting for an answer.</Notice>;
    case "closed":
      return <Notice tone="info" title="This gate is no longer waiting">It was answered, timed out, or its drill stopped.</Notice>;
    default:
      return null;
  }
}

export function GateBody({ state, answer }: Props) {
  if (state.status === "none") {
    return (
      <EmptyState icon={PersonIcon} title="No gate to answer">
        Gates of drill suite runs show on the Progress page. For an <span className="mono">auditaur drill</span> gate,
        ask the agent to show it here.
      </EmptyState>
    );
  }
  return (
    <div className="gate-page">
      <GateStatusBody state={state} answer={answer} />
      <GateSession state={state} />
    </div>
  );
}
