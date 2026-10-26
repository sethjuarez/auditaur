// One test: status, name and labels, recent history, duration, and Run. The row toggles its detail
// (last error, steps, id, and a link to its last trace).
import { ChevronDownIcon, ChevronRightIcon, PlayIcon } from "@primer/octicons-react";
import { Button, Label } from "@primer/react";
import { useId, type MouseEvent } from "react";
import { HistoryDots } from "../../components/HistoryDots";
import { OpenInLinks } from "../../components/OpenIn";
import { StatusIcon } from "../../components/StatusIcon";
import { Tags } from "../../components/Tags";
import { ms } from "../../lib/format";
import type { ExplorerTest } from "./types";

interface Props {
  test: ExplorerTest;
  open: boolean;
  disabled: boolean;
  run: () => void;
  toggle: () => void;
}

function TestDetail({ id, test }: { id: string; test: ExplorerTest }) {
  return (
    <div className="detail" id={id}>
      {test.last?.error && <pre className="detail-error">{test.last.error}</pre>}
      {test.steps.length > 0 && (
        <ol className="detail-steps">{test.steps.map((step, index) => <li key={index}>{step.title}</li>)}</ol>
      )}
      <div className="row">
        <span className="muted path">{test.title} · {test.id}</span>
        {test.last?.runId && (
          <span className="row detail-links">
            <OpenInLinks links={[{ to: "trace", runId: test.last.runId, testId: test.id, label: "Last trace" }]} />
          </span>
        )}
      </div>
    </div>
  );
}

// The whole row toggles on click; keyboard and screen readers use the disclosure button, whose
// native activation click bubbles to the row. Run is a sibling, not nested in the disclosure.
export function TestRow({ test, open, disabled, run, toggle }: Props) {
  const onRun = (event: MouseEvent) => {
    event.stopPropagation();
    run();
  };
  const Chevron = open ? ChevronDownIcon : ChevronRightIcon;
  const detailId = useId();
  return (
    <div className={`box-row test-item${open ? " open" : ""}`}>
      <div className="test clickable" data-id={test.id} onClick={toggle}>
        <button type="button" className="test-toggle" aria-expanded={open} aria-controls={open ? detailId : undefined}>
          <Chevron size={16} className="chevron" />
          <StatusIcon status={test.last?.result ?? "notRun"} />
          <span className="test-name">
            <span className="test-title">{test.name}</span>
            <Tags tags={test.tags} />
            {test.flaky && <Label size="small" variant="attention">flaky</Label>}
            {test.slow && <Label size="small">slow</Label>}
            {test.mode && test.mode !== "run" && <Label size="small" variant="accent" title={test.modeReason}>{test.mode}</Label>}
          </span>
        </button>
        <HistoryDots history={test.history} />
        <span className="duration muted nums">{ms(test.last?.durationMs)}</span>
        <Button size="small" variant="invisible" leadingVisual={PlayIcon} data-run-test={test.id} disabled={disabled} aria-label={`Run ${test.title}`} onClick={onRun}>
          Run
        </Button>
      </div>
      {open && <TestDetail id={detailId} test={test} />}
    </div>
  );
}
