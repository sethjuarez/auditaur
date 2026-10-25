// Meter component: a labelled "n of m" count with a proportional bar.
"use strict";

function meter(label, done, total) {
  const pct = total ? Math.round((done / total) * 100) : 0;
  return '<div class="meter" role="meter" aria-label="' + esc(label) + '" aria-valuemin="0" aria-valuemax="' + total + '" aria-valuenow="' + done + '">' +
    '<div class="meter-label"><span>' + esc(label) + '</span><span class="muted">' + done + " of " + total + (total ? " · " + pct + "%" : "") + "</span></div>" +
    '<div class="meter-track"><div class="meter-fill" style="width:' + pct + '%"></div></div></div>';
}
