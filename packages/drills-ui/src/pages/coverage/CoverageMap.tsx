// Tests against every command and event, a mark where a test touched one.
import { Matrix, type MatrixAxis } from "../../components/Matrix";
import type { Coverage } from "./types";

interface Row extends MatrixAxis {
  touched: Set<string>;
}

export function CoverageMap({ coverage, missedOnly }: { coverage: Coverage; missedOnly: boolean }) {
  const cols: MatrixAxis[] = [
    ...coverage.commands.map((item) => ({ key: `c:${item.command}`, label: item.command, title: `command ${item.command}`, muted: !item.tests.length })),
    ...coverage.events.map((item) => ({ key: `e:${item.event}`, label: item.event, title: `event ${item.event}`, muted: !item.tests.length })),
  ].filter((col) => !missedOnly || col.muted);
  const rows: Row[] = coverage.tests.map((test) => ({
    key: test.id,
    label: test.title,
    touched: new Set([...test.commands.map((name) => `c:${name}`), ...test.events.map((name) => `e:${name}`)]),
  }));
  return (
    <Matrix
      rows={rows}
      cols={cols}
      caption="Which tests exercised which commands and events"
      corner="test \ command, event"
      markLabel="exercised"
      mark={(row, col) => (row.touched.has(col.key) ? `${row.label} → ${col.title}` : null)}
    />
  );
}
