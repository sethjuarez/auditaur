// The IPC commands list: calls, failures, (when comparing) how often the app made each call, and
// (when the app's commands are listed) calls to commands the list does not name.
import { plural } from "../../lib/format";
import { SurfaceRow } from "./SurfaceRow";
import type { CommandCoverage } from "./types";

interface Props {
  items: CommandCoverage[];
  titles: Map<string, string>;
  comparing: boolean;
  registry: boolean;
}

export function CommandList({ items, titles, comparing, registry }: Props) {
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
            registry && !item.registered ? <span title="Called, but not in the app's registered commands: a plugin command, or one the list misses">not registered</span> : null,
          ]}
        />
      ))}
    </ul>
  );
}
