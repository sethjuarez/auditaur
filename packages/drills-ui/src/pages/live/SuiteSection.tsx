// One suite of the run as a bordered list: status, name, file, and time, then its hooks and tests.
import { StatusIcon } from "../../components/StatusIcon";
import { ms } from "../../lib/format";
import { HooksRow } from "./HooksRow";
import { LiveTestRow } from "./LiveTestRow";
import { isOpen, type LiveSuite } from "./types";
import "../../components/Box.css";

const NEEDS_LOOK = new Set(["running", "failed", "interrupted"]);

interface Props {
  suite: LiveSuite;
  runId: string;
  expanded: Record<string, boolean>;
  waitingTests: Set<string>;
  toggle: (id: string, open: boolean) => void;
}

export function SuiteSection({ suite, runId, expanded, waitingTests, toggle }: Props) {
  const hooksId = `hooks:${suite.id}`;
  const hooksOpen = isOpen(expanded, hooksId, suite.hooks.some((step) => step.status !== "passed"));
  return (
    <section className="suite box" aria-label={suite.suite}>
      <div className="box-header suite-head">
        <StatusIcon status={suite.status} />
        <span className="box-title suite-name">{suite.suite}</span>
        <span className="path muted suite-path">{suite.path}</span>
        <span className="spacer" />
        <span className="duration muted nums">{ms(suite.durationMs)}</span>
      </div>
      {suite.error && <div className="suite-error"><pre className="detail-error">{suite.error}</pre></div>}
      {suite.hooks.length > 0 && <HooksRow id={hooksId} hooks={suite.hooks} open={hooksOpen} toggle={() => toggle(hooksId, hooksOpen)} />}
      {suite.tests.map((test) => {
        const open = isOpen(expanded, test.id, NEEDS_LOOK.has(test.result));
        return (
          <LiveTestRow
            key={test.id}
            test={test}
            runId={runId}
            waiting={waitingTests.has(test.title)}
            open={open}
            toggle={() => toggle(test.id, open)}
          />
        );
      })}
    </section>
  );
}