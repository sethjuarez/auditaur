// Helpers shared by every drill canvas page. Loaded as a classic script before the page's own.
"use strict";

const ICONS = { passed: "✓", failed: "✗", timedOut: "⏱", flaky: "◐", interrupted: "!", running: "●", skipped: "⊘", notRun: "·", pending: "·" };

const esc = (value) => String(value ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

// Milliseconds as "840 ms" or "1.2 s".
function ms(raw, digits = 1) {
  const value = Number(raw);
  if (raw == null || Number.isNaN(value)) return "";
  return value < 1000 ? Math.round(value) + " ms" : (value / 1000).toFixed(digits) + " s";
}

// Milliseconds since an ISO timestamp, or null.
function since(ts) {
  const start = Date.parse(ts);
  return Number.isNaN(start) ? null : Math.max(0, Date.now() - start);
}

const cls = (status) => (ICONS[status] ? status : "notRun");
const icon = (status, title) => '<span class="icon ' + cls(status) + '" title="' + esc(title ?? status) + '">' + ICONS[cls(status)] + "</span>";

// The panel server answers `{ error }` with a non-2xx status for anything it refuses.
async function request(path, init) {
  const res = await fetch(path, init);
  const value = await res.json();
  if (!res.ok || value?.error) throw new Error(value?.error ?? res.statusText);
  return value;
}

const getJson = (path) => request(path);
const postJson = (path, body) =>
  request(path, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body ?? {}) });

// Replaces a <select>'s options only when they change, and leaves it alone while focused.
function options(el, html, value) {
  if (el.dataset.options !== html) {
    el.innerHTML = html;
    el.dataset.options = html;
  }
  if (document.activeElement !== el) el.value = value;
}