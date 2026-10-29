// Every gate the run is waiting on. A new gate takes focus at its first field.
import { useEffect, useRef } from "react";
import { GateForm, type Gate, type GateAnswer } from "../../components/gate";

export function GateList({ gates, answer }: { gates: Gate[]; answer: (answer: GateAnswer) => Promise<boolean> }) {
  const list = useRef<HTMLDivElement>(null);
  const keys = gates.map((gate) => `${gate.requestId}:${gate.since}`);
  const signature = keys.join("|");
  useEffect(() => {
    if (signature) list.current?.querySelector<HTMLElement>("input, textarea")?.focus();
  }, [signature]);
  return (
    <div id="gates" className="gates" ref={list} aria-live="assertive">
      {gates.map((gate, index) => <GateForm key={keys[index]} gate={gate} answer={answer} />)}
    </div>
  );
}