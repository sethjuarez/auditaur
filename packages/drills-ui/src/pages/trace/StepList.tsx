// The attempt's steps as a single-select listbox. Clicking a step focuses it and clicking it again
// goes back to the whole attempt; arrow keys, Home, and End move the selection, Enter or Space
// toggles it, and only one step is in the tab order (the selected one, else the first).
import { useRef, type KeyboardEvent, type MouseEvent } from "react";
import { StepRow } from "./StepRow";
import type { TraceSelected } from "./types";

interface Props {
  selected: TraceSelected;
  step: number | null;
  // A step to show, or "all" for the whole attempt.
  pick: (step: number | "all") => void;
}

const indexOf = (target: EventTarget | null) => {
  const option = (target as Element | null)?.closest?.<HTMLElement>("[role=option][data-step]");
  return option ? Number(option.dataset.step) : null;
};

export function StepList({ selected, step, pick }: Props) {
  const rows = useRef<(HTMLDivElement | null)[]>([]);
  const count = selected.steps.length;
  if (!count) return <div className="steps-empty muted">No steps recorded.</div>;
  const toggle = (index: number) => pick(index === step ? "all" : index);
  const move = (index: number) => {
    const next = Math.max(0, Math.min(count - 1, index));
    rows.current[next]?.focus();
    if (next !== step) pick(next);
  };
  const onKeyDown = (event: KeyboardEvent) => {
    const current = indexOf(event.target) ?? step ?? 0;
    const keys: Record<string, () => void> = {
      ArrowDown: () => move(current + 1),
      ArrowUp: () => move(current - 1),
      Home: () => move(0),
      End: () => move(count - 1),
      Enter: () => toggle(current),
      " ": () => toggle(current),
    };
    const handler = keys[event.key];
    if (!handler) return;
    event.preventDefault();
    handler();
  };
  const onClick = (event: MouseEvent) => {
    const index = indexOf(event.target);
    if (index != null) toggle(index);
  };
  const focusable = step ?? 0;
  return (
    <div
      className="steps"
      role="listbox"
      aria-label="Steps: select one to see its screenshots and telemetry"
      onKeyDown={onKeyDown}
      onClick={onClick}
    >
      {selected.steps.map((each) => (
        <StepRow
          key={each.index}
          ref={(node) => { rows.current[each.index] = node; }}
          step={each}
          events={selected.events.filter((event) => event.step === each.index)}
          selected={step === each.index}
          focusable={focusable === each.index}
        />
      ))}
    </div>
  );
}
