// Checklist component: checkbox rows narrowed by a filter query.
// Items: { value, label, detail?, badge? }. Pages own the chosen set and handle `change` on `[data-check]`.
"use strict";

function checklist(items, chosen, query) {
  const needle = (query ?? "").trim().toLowerCase();
  const shown = needle
    ? items.filter((item) => [item.label, item.detail, item.value].some((text) => text?.toLowerCase().includes(needle)))
    : items;
  const rows = shown.map((item) =>
    '<label class="checklist-row"><input type="checkbox" data-check="' + esc(item.value) + '"' + (chosen.has(item.value) ? " checked" : "") + "> " +
    "<span>" + esc(item.label) + "</span>" +
    (item.detail ? ' <span class="path muted" title="' + esc(item.value) + '">' + esc(item.detail) + "</span>" : "") +
    (item.badge ? ' <span class="badge">' + esc(item.badge) + "</span>" : "") + "</label>");
  const count = needle ? shown.length + " of " + items.length : String(items.length);
  return '<div class="checklist-count muted">' + count + " · " + chosen.size + " chosen</div>" +
    '<div class="checklist">' + (rows.length ? rows.join("") : '<span class="muted">Nothing matches.</span>') + "</div>";
}
