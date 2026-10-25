// Comparison views for snapshot results: expected and actual side by side, an overlay slider, the
// pixel diff, colored line diffs, and lazily loaded text files. A classic script after common.js.
"use strict";

const COMPARE_MODES = { side: "Side by side", slider: "Slider", diff: "Diff" };

function compareModes(mode) {
  return Object.entries(COMPARE_MODES).map(([key, label]) =>
    '<button data-compare-mode="' + key + '" aria-pressed="' + (key === mode) + '">' + esc(label) + "</button>").join("");
}

const compareFigure = (url, label) =>
  '<figure class="compare-figure"><img alt="' + esc(label) + '" src="' + esc(url) + '"><figcaption class="muted">' + esc(label) + "</figcaption></figure>";

// `urls` holds `expected`, `actual`, and `diff` image URLs; any may be missing. `split` is the
// slider's percent of expected; the page saves a moved slider on `change`.
function imageCompare(urls, mode, split = 50) {
  if (!urls.actual) return '<div class="muted">The run saved no actual screenshot.</div>';
  if (!urls.expected) return compareFigure(urls.actual, "actual (no baseline yet)");
  if (mode === "diff") return urls.diff ? compareFigure(urls.diff, "diff") : '<div class="muted">No diff image.</div>';
  if (mode === "slider") {
    return '<div class="compare-slider" style="--split:' + split + '%">' +
      '<img alt="actual" src="' + esc(urls.actual) + '"><img class="compare-top" alt="expected" src="' + esc(urls.expected) + '">' +
      '<input type="range" class="compare-range" min="0" max="100" value="' + split + '" aria-label="Expected and actual split"></div>' +
      '<div class="row compare-legend muted"><span>◀ expected</span><span class="spacer"></span><span>actual ▶</span></div>';
  }
  return '<div class="compare-side">' + compareFigure(urls.expected, "expected") + compareFigure(urls.actual, "actual") + "</div>";
}

// Colors `- expected` and `+ actual` lines of a line diff.
function lineDiff(text) {
  return '<pre class="compare-lines mono">' + String(text ?? "").split("\n").map((line) => {
    const kind = line.startsWith("- ") ? " del" : line.startsWith("+ ") ? " ins" : "";
    return '<span class="line' + kind + '">' + (esc(line) || " ") + "</span>";
  }).join("") + "</pre>";
}

// A collapsed text file that loads when opened.
const textFile = (label, url) =>
  '<details class="compare-text" data-src="' + esc(url) + '"><summary>' + esc(label) + '</summary><pre class="mono">Loading…</pre></details>';

document.addEventListener("input", (event) => {
  const range = event.target.closest?.(".compare-range");
  if (!range) return;
  range.parentElement.style.setProperty("--split", range.value + "%");
});

document.addEventListener("toggle", async (event) => {
  const details = event.target;
  if (!details.open || !details.classList?.contains("compare-text") || details.dataset.loaded) return;
  details.dataset.loaded = "1";
  const pre = details.querySelector("pre");
  try {
    const res = await fetch(details.dataset.src);
    const text = await res.text();
    if (!res.ok) throw new Error(JSON.parse(text).error);
    pre.textContent = text;
  } catch (err) {
    pre.textContent = "Could not load: " + err.message;
  }
}, true);
