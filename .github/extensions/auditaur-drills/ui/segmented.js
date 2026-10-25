// Segmented control component: one toggle button per item, the current one pressed.
// Pages handle clicks on `[data-segment]` and read `dataset.segmentGroup` / `dataset.segment`.
"use strict";

function segmented(group, items, value) {
  return '<span class="segmented" role="group" aria-label="' + esc(group) + '">' +
    items.map(([key, label]) =>
      '<button data-segment-group="' + esc(group) + '" data-segment="' + esc(key) + '" aria-pressed="' + (key === value) + '">' + esc(label) + "</button>").join("") +
    "</span>";
}
