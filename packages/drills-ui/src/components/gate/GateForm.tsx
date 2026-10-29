// A gate waiting for an answer: what it asks, the fields it needs, and its answers. A retry answer
// skips the required-field check, as the runner does.
import { ClockIcon } from "@primer/octicons-react";
import { Label } from "@primer/react";
import { useState, type FormEvent } from "react";
import { GateChoices } from "./GateChoices";
import { GateField } from "./GateField";
import { GateMeta } from "./GateMeta";
import type { Gate, GateAnswer } from "./types";
import "./Gate.css";

interface Props {
  gate: Gate;
  answer: (answer: GateAnswer) => Promise<boolean>;
}

export function GateForm({ gate, answer }: Props) {
  // Answered stays disabled until the gate leaves the list; a refused or retry answer re-arms it.
  const [locked, setLocked] = useState(false);
  const [missing, setMissing] = useState<string[]>([]);
  const onSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const form = event.currentTarget;
    const button = (event.nativeEvent as SubmitEvent).submitter as HTMLButtonElement | null;
    const retry = button?.dataset.outcome === "retry";
    if (!gate.requestId) return;
    const fields = [...form.querySelectorAll<HTMLInputElement | HTMLTextAreaElement>("input[name], textarea[name]")];
    const inputs = fields.map((field) => ({ id: field.name, value: field.value }));
    const empty = retry ? [] : gate.inputs.filter((input) => input.required && !inputs.find((x) => x.id === input.id)?.value.trim()).map((input) => input.id);
    setMissing(empty);
    if (empty.length) {
      fields.find((field) => field.name === empty[0])?.focus();
      return;
    }
    setLocked(true);
    const ok = await answer({ requestId: gate.requestId, choiceId: button?.value || null, inputs });
    if (!ok || retry) setLocked(false);
  };
  return (
    <form className="gate" noValidate data-request={gate.requestId ?? ""} onSubmit={(event) => void onSubmit(event)}>
      <div className="gate-head">
        <ClockIcon size={16} className="gate-icon" />
        <h2>{gate.name}</h2>
        <Label size="small" variant="attention">Waiting for you</Label>
      </div>
      <div className="gate-body">
        <GateMeta gate={gate} />
        {gate.inputs.length > 0 && <div className="gate-fields">{gate.inputs.map((input) => <GateField key={input.id} input={input} missing={missing.includes(input.id)} onEdit={() => setMissing((ids) => ids.filter((id) => id !== input.id))} />)}</div>}
        {gate.requestId
          ? <GateChoices choices={gate.choices} disabled={locked} />
          : <p className="muted gate-note">This gate has no answer queue; answer it in the terminal running the drill.</p>}
      </div>
    </form>
  );
}