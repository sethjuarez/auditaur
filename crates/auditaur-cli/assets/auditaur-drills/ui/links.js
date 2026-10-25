// "Open in" links from one drill canvas to another (component: links). Each link posts the panel's
// `/open` (lib/navigator.mjs), which opens or focuses the target canvas on the same run or test.
"use strict";

const OPEN_IN_LABELS = { run: "Run", trace: "Trace", snapshots: "Snapshots", coverage: "Coverage" };

// One link: `{ to, runId?, testId?, label? }`.
function openInLink(link) {
  const label = link.label ?? OPEN_IN_LABELS[link.to];
  return '<button class="open-in-link" data-open-to="' + esc(link.to) + '"' +
    (link.runId ? ' data-open-run="' + esc(link.runId) + '"' : "") +
    (link.testId ? ' data-open-test="' + esc(link.testId) + '"' : "") +
    ' title="Open the ' + esc(OPEN_IN_LABELS[link.to].toLowerCase()) + " canvas" + (link.runId ? " for " + esc(link.runId) : "") + '">' +
    esc(label) + ' <span aria-hidden="true">↗</span></button>';
}

// A labelled group of links, or "" when there are none.
function openInLinks(links) {
  return links.length
    ? '<span class="open-in" role="group" aria-label="Open in">' + links.map(openInLink).join("") + "</span>"
    : "";
}

// Links to every other run canvas for `runId`, from the page `here` (a link name, or null).
function runLinks(runId, here) {
  return runId ? Object.keys(OPEN_IN_LABELS).filter((to) => to !== here).map((to) => ({ to, runId })) : [];
}

// Fills `el` only when its links change, so a poll doesn't replace a button under the pointer.
function showOpenIn(el, links) {
  const html = openInLinks(links);
  if (el.dataset.links !== html) {
    el.innerHTML = html;
    el.dataset.links = html;
  }
}

function openInToast(message) {
  let toast = document.getElementById("open-in-toast");
  if (!toast) {
    toast = document.createElement("div");
    toast.id = "open-in-toast";
    toast.className = "banner error";
    toast.setAttribute("role", "alert");
    document.body.append(toast);
  }
  toast.textContent = message;
  toast.hidden = false;
  clearTimeout(openInToast.timer);
  openInToast.timer = setTimeout(() => { toast.hidden = true; }, 6000);
}

// Capture phase: a link inside a clickable row opens the canvas without also toggling the row.
document.addEventListener("click", async (event) => {
  const link = event.target.closest?.("[data-open-to]");
  if (!link) return;
  event.stopPropagation();
  if (link.disabled) return;
  link.disabled = true;
  try {
    await postJson("/open", {
      to: link.dataset.openTo,
      ...(link.dataset.openRun && { runId: link.dataset.openRun }),
      ...(link.dataset.openTest && { testId: link.dataset.openTest }),
    });
  } catch (error) {
    openInToast("Could not open " + (OPEN_IN_LABELS[link.dataset.openTo] ?? "canvas").toLowerCase() + ": " + error.message);
  } finally {
    link.disabled = false;
  }
}, true);
