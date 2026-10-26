// The events list: records, directions seen, and (when comparing) how often the app recorded each.
import { plural } from "../../lib/format";
import { SurfaceRow } from "./SurfaceRow";
import type { EventCoverage } from "./types";

interface Props {
  items: EventCoverage[];
  titles: Map<string, string>;
  comparing: boolean;
}

export function EventList({ items, titles, comparing }: Props) {
  return (
    <ul className="box coverage-list" aria-label="Events">
      {items.map((item) => (
        <SurfaceRow
          key={item.event}
          name={item.event}
          testIds={item.tests}
          titles={titles}
          stats={[
            plural(item.count, "record"),
            item.directions.join("/"),
            comparing ? `${plural(item.seen, "record")} in app` : null,
          ]}
        />
      ))}
    </ul>
  );
}
