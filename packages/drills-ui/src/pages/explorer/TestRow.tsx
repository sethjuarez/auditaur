// One test: status, name and labels, recent history, duration, and a run button. The row toggles its detail
// (last error, steps, id, and a link to its last trace).
import { PlayIcon } from "@primer/octicons-react";
import { IconButton, Label } from "@primer/react";
import type { MouseEvent } from "react";
import { DisclosureRow } from "../../components/DisclosureRow";
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

function TestDetail({ test }: { test: ExplorerTest }) {
  return (
    <>
      {test.last?.error && <pre className="detail-error">{test.last.error}</pre>}
      {test.steps.length > 0 && (
        <ol className="detail-steps">{test.steps.map((step, index) => <li key={index}>{step.title}</li>)}</ol>
      )}
      <div className="row">
        <span className="muted path">{test.title} · {test.id}</span>
        {test.last?.runId && (
          <span className="row detail-links">
            <OpenInLinks caption="Open this test's last run in" links={[{ to: "trace", runId: test.last.runId, testId: test.id }]} />
          </span>
        )}
      </div>
    </>
  );
}

export function TestRow({ test, open, disabled, run, toggle }: Props) {
  const onRun = (event: MouseEvent) => {
    event.stopPropagation();
    run();
  };
  const name = (
    <>
      <span className="test-title">{test.name}</span>
      <Tags tags={test.tags} />
      {test.flaky && <Label size="small" variant="attention">flaky</Label>}
      {test.slow && <Label size="small">slow</Label>}
      {test.mode && test.mode !== "run" && <Label size="small" variant="accent" title={test.modeReason}>{test.mode}</Label>}
    </>
  );
  const trailing = (
    <>
      <HistoryDots history={test.history} />
      <span className="duration muted nums">{ms(test.last?.durationMs)}</span>
      <IconButton size="small" variant="invisible" icon={PlayIcon} className="test-run" data-run-test={test.id} disabled={disabled} aria-label={`Run ${test.title}`} description="Run this test" onClick={onRun} />
    </>
  );
  return (
    <DisclosureRow id={test.id} open={open} toggle={toggle} status={<StatusIcon status={test.last?.result ?? "notRun"} />} name={name} trailing={trailing}>
      <TestDetail test={test} />
    </DisclosureRow>
  );
}