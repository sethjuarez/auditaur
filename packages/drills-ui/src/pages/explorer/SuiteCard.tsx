// One suite as a bordered list: its name, tags, file, and failing count, then its matching tests.
import { PlayIcon } from "@primer/octicons-react";
import { Button, CounterLabel } from "@primer/react";
import { Tags } from "../../components/Tags";
import { TestRow } from "./TestRow";
import type { ExplorerSuite } from "./types";
import "../../components/Box.css";

interface Props {
  suite: ExplorerSuite;
  expanded: string[];
  disabled: boolean;
  run: (path: string, body?: unknown) => Promise<void>;
  toggle: (id: string) => void;
}

export function SuiteCard({ suite, expanded, disabled, run, toggle }: Props) {
  const failing = suite.tests.filter((test) => test.last?.result === "failed").length;
  return (
    <section className="suite box" aria-label={suite.suite}>
      <div className="box-header suite-head">
        <span className="box-title suite-name">{suite.suite}</span>
        <Tags tags={suite.tags} />
        <span className="path muted suite-path">{suite.path}</span>
        <span className="spacer" />
        {failing > 0 && <CounterLabel scheme="primary" className="failing">{`${failing} failing`}</CounterLabel>}
        <Button size="small" leadingVisual={PlayIcon} data-run-suite={suite.suite} disabled={disabled} onClick={() => void run("/run", { suite: suite.suite })}>
          Run suite
        </Button>
      </div>
      {suite.tests.filter((test) => test.shown).map((test) => (
        <TestRow
          key={test.id}
          test={test}
          open={expanded.includes(test.id)}
          disabled={disabled}
          run={() => void run("/run", { test: test.id })}
          toggle={() => toggle(test.id)}
        />
      ))}
    </section>
  );
}
