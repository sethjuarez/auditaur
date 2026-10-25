// Matrix component: a rows × columns grid of marks, such as tests × IPC commands.
// `rows` and `cols` are `{ key, label, cls?, title? }`; `cell(row, col)` returns `{ cls, title }` or null.
"use strict";

function matrix({ rows, cols, cell, corner = "" }) {
  if (!rows.length || !cols.length) return '<div class="empty muted">Nothing to map.</div>';
  const head = '<tr><th class="matrix-corner muted">' + esc(corner) + "</th>" +
    cols.map((col) => '<th class="matrix-col ' + esc(col.cls ?? "") + '" scope="col" title="' + esc(col.title ?? col.label) + '"><span>' + esc(col.label) + "</span></th>").join("") + "</tr>";
  const body = rows.map((row) =>
    '<tr><th class="matrix-row ' + esc(row.cls ?? "") + '" scope="row" title="' + esc(row.title ?? row.label) + '">' + esc(row.label) + "</th>" +
    cols.map((col) => {
      const mark = cell(row, col);
      return mark
        ? '<td class="matrix-cell ' + esc(mark.cls ?? "") + '" title="' + esc(mark.title ?? "") + '">●</td>'
        : '<td class="matrix-cell"></td>';
    }).join("") + "</tr>").join("");
  return '<div class="matrix-wrap"><table class="matrix"><thead>' + head + "</thead><tbody>" + body + "</tbody></table></div>";
}
