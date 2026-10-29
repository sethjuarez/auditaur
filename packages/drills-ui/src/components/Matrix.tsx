// A rows × columns grid of marks, such as tests × IPC commands. `mark(row, col)` names a marked cell
// (its tooltip and accessible text) or returns null for an empty one.
import { DotFillIcon } from "@primer/octicons-react";
import "./Matrix.css";

export interface MatrixAxis {
  key: string;
  label: string;
  title?: string;
  muted?: boolean;
}

interface Props<R extends MatrixAxis, C extends MatrixAxis> {
  rows: R[];
  cols: C[];
  mark: (row: R, col: C) => string | null;
  caption: string;
  corner: string;
  // What a mark means to a screen reader, e.g. "exercised".
  markLabel: string;
}

export function Matrix<R extends MatrixAxis, C extends MatrixAxis>({ rows, cols, mark, caption, corner, markLabel }: Props<R, C>) {
  return (
    <div className="matrix-wrap">
      <table className="matrix">
        <caption className="visually-hidden">{caption}</caption>
        <thead>
          <tr>
            <th className="matrix-corner muted" scope="col">{corner}</th>
            {cols.map((col) => (
              <th key={col.key} className={`matrix-col${col.muted ? " muted" : ""}`} scope="col" title={col.title ?? col.label}>
                <span>{col.label}</span>
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.key}>
              <th className={`matrix-row${row.muted ? " muted" : ""}`} scope="row" title={row.title ?? row.label}>{row.label}</th>
              {cols.map((col) => {
                const title = mark(row, col);
                return title ? (
                  <td key={col.key} className="matrix-cell marked" title={title}>
                    <DotFillIcon size={16} aria-label={markLabel} />
                  </td>
                ) : <td key={col.key} className="matrix-cell" />;
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
