// One IPC command or event: whether any test exercised it, the tests that did, and its counts.
import { Label } from "@primer/react";
import type { ReactNode } from "react";
import { StatusIcon } from "../../components/StatusIcon";

interface Props {
  name: string;
  testIds: string[];
  titles: Map<string, string>;
  stats: ReactNode[];
}

export function SurfaceRow({ name, testIds, titles, stats }: Props) {
  const hit = testIds.length > 0;
  return (
    <li className={`box-row coverage-item${hit ? "" : " missed"}`}>
      <StatusIcon status={hit ? "passed" : "notRun"} label={hit ? "Exercised" : "Not exercised"} />
      <div className="coverage-item-main">
        <div className="coverage-item-name mono">{name}</div>
        <div className="coverage-item-tests">
          {hit
            ? testIds.map((id) => <Label key={id} size="small" variant="secondary" className="tag">{titles.get(id) ?? id}</Label>)
            : <span className="muted">No test exercised this.</span>}
        </div>
      </div>
      <div className="coverage-item-stats muted nums">
        {stats.filter(Boolean).map((stat, index) => <span key={index}>{stat}</span>)}
      </div>
    </li>
  );
}
