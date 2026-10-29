// The steps section: the kind filter, the time axis, and the step list.
import { KindFilter } from "./KindFilter";
import { StepList } from "./StepList";
import { Timeline } from "./Timeline";
import type { TraceSelected } from "./types";
import "../../components/Box.css";

interface Props {
  selected: TraceSelected;
  kinds: string[];
  step: number | null;
  pick: (step: number | "all") => void;
  toggleKind: (kind: string) => void;
}

export function StepsSection({ selected, kinds, step, pick, toggleKind }: Props) {
  return (
    <section className="box trace-section" aria-labelledby="steps-title">
      <div className="box-header">
        <h2 id="steps-title" className="box-title section-title">Steps</h2>
        <span className="spacer" />
        <KindFilter kinds={kinds} events={selected.events} toggle={toggleKind} />
      </div>
      <Timeline selected={selected} kinds={kinds} step={step} pick={(index) => pick(index === step ? "all" : index)} />
      <StepList selected={selected} step={step} pick={pick} />
    </section>
  );
}
