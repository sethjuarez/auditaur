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

let stateSeq = 0;
let viewEpoch = 0;
// View fields the page changed but the server hasn't confirmed yet: field -> { value, token }.
const pendingView = new Map();

// The panel's `/state` (data plus the view as `ui`), or null when a newer request overtook this one.
// A view change landing mid-request makes it fetch again, and unconfirmed view changes are laid over
// `ui`, so a poll can't undo what the user just did.
async function fetchState(query) {
  const mine = ++stateSeq;
  for (;;) {
    const epoch = viewEpoch;
    const value = await getJson("/state" + (query ?? ""));
    if (mine !== stateSeq) return null;
    if (epoch !== viewEpoch) continue;
    if (value.ui) for (const [name, pending] of pendingView) value.ui[name] = pending.value;
    return value;
  }
}

// Patches the server-owned view; the server then tells every open copy of the page to reload.
async function setView(patch) {
  const token = {};
  for (const [name, value] of Object.entries(patch)) pendingView.set(name, { value, token });
  try {
    return await postJson("/view", patch);
  } finally {
    for (const name of Object.keys(patch)) if (pendingView.get(name)?.token === token) pendingView.delete(name);
    viewEpoch += 1;
  }
}

// Calls `reload` whenever the server says the view or data changed, e.g. because the agent acted.
// A still capture (`?still`, see lib/browser.mjs) skips the stream: an open request would stop
// headless virtual time, and a still image needs no live updates.
function onServerChange(reload) {
  if (new URLSearchParams(location.search).has("still")) return;
  new EventSource("/events").onmessage = () => reload();
}