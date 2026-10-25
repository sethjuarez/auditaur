const KINDS = ["ipc", "event", "log", "error", "span"];
const kinds = new Set(KINDS);
let view = null;
let error = null;
let step = undefined;
let shownKey = null;
let timer = null;

const kindOf = (event) => (event.kind === "span-event" ? "span" : event.kind);
const fileUrl = (path) => "/file?runId=" + encodeURIComponent(view.run.runId) + "&path=" + encodeURIComponent(path);

async function load() {
  clearTimeout(timer);
  try {
    view = await getJson("/state");
    error = null;
  } catch (err) {
    error = "Canvas server unreachable: " + err.message;
  }
  render();
  if (view?.run?.status === "running") timer = setTimeout(load, 2000);
}

async function select(body) {
  try {
    await postJson("/select", body);
  } catch (err) {
    error = err.message;
  }
  await load();
}

function timeline(sel) {
  const ends = sel.steps.map((s) => (s.offsetMs ?? 0) + (s.durationMs ?? 0)).concat(sel.events.map((e) => (e.offsetMs ?? 0) + (e.durationMs ?? 0)));
  const total = Math.max(1, sel.durationMs ?? 0, ...ends);
  const pct = (value) => Math.max(0, Math.min(100, (100 * (value ?? 0)) / total));
  const segs = sel.steps.map((s) => '<div class="seg ' + cls(s.status) + (step === s.index ? " sel" : "") + '" data-step="' + s.index + '" tabindex="0" role="button"' +
    ' title="' + esc(s.title + " · " + ms(s.durationMs, 2)) + '" style="left:' + pct(s.offsetMs) + "%;width:" + Math.max(0.4, pct(s.durationMs)) + '%"></div>').join("");
  const shown = sel.events.filter((e) => kinds.has(kindOf(e)) && e.offsetMs != null);
  const ticks = shown.map((e) => (e.durationMs
    ? '<div class="tick span-bar k-' + esc(e.kind) + '" title="' + esc(e.name + " · " + ms(e.durationMs, 2)) + '" style="left:' + pct(e.offsetMs) + "%;width:" + Math.max(0.3, pct(e.durationMs)) + '%"></div>'
    : "") + '<div class="tick k-' + esc(e.kind) + '" title="' + esc(e.kind + ": " + e.name) + '" style="left:' + pct(e.offsetMs) + '%"></div>').join("");
  return '<div class="timeline" aria-label="Timeline"><div class="lane">' + segs + '</div><div class="lane">' + ticks + "</div>" +
    '<span class="axis muted" style="left:8px">0</span><span class="axis muted" style="right:8px">' + ms(total, 2) + "</span></div>";
}

function attachment(a) {
  const caption = '<figcaption class="muted">' + esc(a.name) + ' · <span class="path">' + esc(a.path) + "</span></figcaption>";
  if (/^image\//.test(a.contentType)) {
    return '<figure><img class="shot" loading="lazy" alt="' + esc(a.name) + '" src="' + esc(fileUrl(a.path)) + '">' + caption + "</figure>";
  }
  return '<details data-path="' + esc(a.path) + '"><summary>' + esc(a.name) + ' <span class="path muted">' + esc(a.path) + "</span></summary><pre class=\"mono\">Loading…</pre></details>";
}

function eventsTable(events) {
  if (!events.length) return '<div class="muted" style="font-size:12px">No telemetry in this window.</div>';
  return "<table>" + events.map((e) => "<tr><td class=\"muted mono at\">+" + ms(e.offsetMs, 2) + '</td><td><span class="kind k-' + esc(e.kind) + '">' + esc(e.kind) + "</span></td>" +
    "<td>" + esc(e.name) + (e.status && e.status !== "Unset" ? ' <span class="badge">' + esc(e.status) + "</span>" : "") + (e.durationMs ? ' <span class="muted">' + ms(e.durationMs, 2) + "</span>" : "") + "</td>" +
    '<td class="detail">' + esc(e.detail) + "</td></tr>").join("") + "</table>";
}

function render() {
  const banners = error ? '<div class="banner error">' + esc(error) + "</div>" : "";
  document.getElementById("banners").innerHTML = banners;
  if (!view) return;
  options(document.getElementById("run"), '<option value="">Latest run</option>' + view.runs.map((r) =>
    '<option value="' + esc(r.runId) + '">' + esc(r.runId) + " · " + esc(r.status) + "</option>").join(""), view.pinned ?? "");
  const sel = view.selected;
  options(document.getElementById("test"), view.entries.map((e) =>
    '<option value="' + esc(e.key) + '">' + ICONS[cls(e.result)] + " " + esc(e.title) + "</option>").join(""), sel?.key ?? "");
  const entry = view.entries.find((e) => e.key === sel?.key);
  const attempts = entry?.attempts ?? [];
  const attemptEl = document.getElementById("attempt");
  attemptEl.hidden = attempts.length < 2;
  options(attemptEl, attempts.filter((a) => Number.isSafeInteger(a.attempt) && a.attempt >= 0).map((a) => '<option value="' + a.attempt + '">Attempt ' + (a.attempt + 1) + " · " + esc(a.status) + "</option>").join(""), String(sel?.attempt ?? ""));

  const main = document.getElementById("main");
  if (!view.run) {
    main.innerHTML = '<div class="empty muted">' + (view.pinned ? "Run " + esc(view.pinned) + " was not found." : 'No drill runs in <span class="path">.auditaur/drill-runs</span> yet.') + "</div>";
    document.getElementById("summary").textContent = "";
    return;
  }
  if (!sel) {
    main.innerHTML = '<div class="empty muted">This run has no tests.</div>';
    return;
  }
  const key = sel.key + "|" + sel.attempt;
  if (key !== shownKey) {
    shownKey = key;
    const failing = sel.steps.find((s) => ["failed", "timedOut"].includes(s.status));
    step = failing ? failing.index : null;
  }
  document.getElementById("summary").innerHTML = ['<span class="' + cls(sel.status) + '">' + esc(sel.status) + "</span>", ms(sel.durationMs, 2),
    sel.steps.length + " steps", sel.events.length + " telemetry records"].filter(Boolean).join(" · ");

  const counts = (index) => {
    const mine = sel.events.filter((e) => e.step === index);
    return KINDS.map((k) => { const n = mine.filter((e) => kindOf(e) === k).length; return n ? '<span class="badge k-' + k + '" style="background:none">' + n + " " + k + "</span>" : ""; }).join("");
  };
  const chosen = sel.steps[step];
  const scoped = sel.events.filter((e) => kinds.has(kindOf(e)) && (step == null || e.step === step));
  const before = sel.events.filter((e) => e.step == null).length;
  main.innerHTML =
    (sel.missing ? '<div class="banner muted">This attempt has not written its report yet.</div>' : "") +
    (sel.error ? '<div class="error" style="margin-top:8px">' + esc(sel.error) + "</div>" : "") +
    '<div class="row" style="margin-top:10px">' + KINDS.map((k) => '<button data-kind="' + k + '" aria-pressed="' + kinds.has(k) + '"><span class="kind k-' + k + '" style="background:none">●</span> ' + k + "</button>").join("") + "</div>" +
    timeline(sel) +
    '<div class="steps">' + sel.steps.map((s) => '<div class="step' + (step === s.index ? " sel" : "") + '" data-step="' + s.index + '" role="button" tabindex="0" aria-pressed="' + (step === s.index) + '">' +
      icon(s.status) + '<span class="muted mono">+' + ms(s.offsetMs, 2) + "</span><span>" + (s.phase && s.phase !== "test" ? '<span class="muted">' + esc(s.phase) + "</span> " : "") + esc(s.title) +
      (s.soft ? '<span class="badge">soft</span>' : "") + (s.attachments.length ? '<span class="badge">📎 ' + s.attachments.length + "</span>" : "") + "</span>" +
      "<span>" + counts(s.index) + '</span><span class="duration muted">' + ms(s.durationMs, 2) + "</span></div>").join("") + "</div>" +
    (chosen
      ? "<h2>Step " + (chosen.index + 1) + ": " + esc(chosen.title) + ' <button data-step="all">Show all</button></h2>' +
        (chosen.error ? '<div class="error">' + esc(chosen.error) + "</div>" : "") +
        chosen.attachments.map(attachment).join("") +
        (chosen.attachments.length ? "" : '<div class="muted" style="font-size:12px">No screenshots for this step. Run with <span class="path">--trace on</span> to capture every step.</div>')
      : "<h2>Whole " + (sel.kind === "hooks" ? "suite" : "attempt") + "</h2>" + (before ? '<div class="muted" style="font-size:12px">' + before + " records arrived before the first step.</div>" : "") +
        sel.attachments.map(attachment).join("")) +
    "<h2>Telemetry</h2>" + eventsTable(scoped) +
    (sel.filterNotes && (typeof sel.filterNotes === "string" || Object.keys(sel.filterNotes).length) ? '<div class="muted" style="font-size:11px;margin-top:6px">' + esc(typeof sel.filterNotes === "string" ? sel.filterNotes : JSON.stringify(sel.filterNotes)) + "</div>" : "");
}

document.addEventListener("click", (event) => {
  const kind = event.target.closest("[data-kind]");
  if (kind) {
    kinds.has(kind.dataset.kind) ? kinds.delete(kind.dataset.kind) : kinds.add(kind.dataset.kind);
    return render();
  }
  const target = event.target.closest("[data-step]");
  if (target) {
    const value = target.dataset.step;
    step = value === "all" || Number(value) === step ? null : Number(value);
    return render();
  }
  if (event.target.id === "refresh") load();
});
document.addEventListener("keydown", (event) => {
  const row = event.target.closest?.("[data-step][tabindex]");
  if (row && (event.key === "Enter" || event.key === " ")) {
    event.preventDefault();
    row.click();
  }
});
document.addEventListener("toggle", async (event) => {
  const details = event.target;
  if (!details.open || !details.dataset?.path || details.dataset.loaded) return;
  details.dataset.loaded = "1";
  const pre = details.querySelector("pre");
  try {
    const res = await fetch(fileUrl(details.dataset.path));
    const text = await res.text();
    if (!res.ok) throw new Error(JSON.parse(text).error);
    let shown = text;
    try { shown = JSON.stringify(JSON.parse(text), null, 2); } catch {}
    pre.textContent = shown.length > 200000 ? shown.slice(0, 200000) + "\n…" : shown;
  } catch (err) {
    pre.textContent = "Could not load: " + err.message;
  }
}, true);
document.getElementById("run").addEventListener("change", (event) => select({ runId: event.target.value || null }));
document.getElementById("test").addEventListener("change", (event) => select({ runId: view?.pinned ?? null, testId: event.target.value }));
document.getElementById("attempt").addEventListener("change", (event) => select({ runId: view?.pinned ?? null, testId: view?.selected?.key, attempt: Number(event.target.value) }));
load();
