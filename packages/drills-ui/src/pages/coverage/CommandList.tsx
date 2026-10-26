// The IPC commands list: calls, failures, and (when comparing) how often the app made each call.
import { plural } from "../../lib/format";
import { SurfaceRow } from "./SurfaceRow";
import type { CommandCoverage } from "./types";

interface Props {
  items: CommandCoverage[];
  titles: Map<string, string>;
  comparing: boolean;
}

export function CommandList({ items, titles, comparing }: Props) {
  return (
    <ul className="box coverage-list" aria-label="IPC commands">
      {items.map((item) => (
        <SurfaceRow
          key={item.command}
          name={item.command}
          testIds={item.tests}
          titles={titles}
          stats={[
            plural(item.calls, "call"),
            item.failed ? <span className="status-text failed">{`${item.failed} failed`}</span> : null,
            comparing ? `${plural(item.seen, "record")} in app` : null,
          ]}
        />
      ))}
    </ul>
  );
}
