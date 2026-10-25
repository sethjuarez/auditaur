const STATUS = {
  changed: { cls: "failed", label: "Changed" },
  missing: { cls: "failed", label: "No baseline" },
  accepted: { cls: "passed", label: "Accepted" },
  noActual: { cls: "skipped", label: "No actual output" },
  unstable: { cls: "flaky", label: "Never stabilized" },
  unreadable: { cls: "failed", label: "Baseline unreadable" },
};
const ACCEPTABLE = ["changed", "missing"];
let view = null;
let error = null;
let notice = null;
let busy = false;
let timer = null;

const mode = () => view?.ui.mode ?? "side";
const fileUrl = (path) => "/file?runId=" + encodeURIComponent(view.run.runId) + "&path=" + encodeURIComponent(path);
const locked = () => busy || view?.run?.status === "running";
const urlOf = (item, name) => (item.files[name] ? fileUrl(item.files[name].path) : null);

async function load() {
  clearTimeout(timer);
  try {
    const next = await fetchState();
    if (!next) return;
    view = next;
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
    notice = null;
  } catch (err) {
    error = err.message;
  }
  await load();
}

async function accept(ids) {
  if (!ids.length || busy) return;
  busy = true;
  render();
  try {
    const result = await postJson("/accept", { runId: view.run.runId, ids });
    notice = "Accepted " + result.accepted.length + " baseline" + (result.accepted.length === 1 ? "" : "s") + ". Re-run the drills to confirm.";
    error = null;
  } catch (err) {
    error = err.message;
  }
  busy = false;
  await load();
}

function body(item) {
  if (item.kind === "screenshot") {
    return '<div class="snap-message muted">' + esc(item.message) + "</div>" +
      '<div class="snap-body">' + imageCompare({ expected: urlOf(item, "expected"), actual: urlOf(item, "actual"), diff: urlOf(item, "diff") }, mode()) + "</div>";
  }
  const files = ["expected", "actual"].filter((name) => item.files[name]).map((name) => textFile(name, urlOf(item, name))).join("");
  return '<div class="snap-body">' + lineDiff(item.message) + files + "</div>";
}

function card(item) {
  const status = STATUS[item.status] ?? STATUS.changed;
  const suite = String(item.test ?? "").startsWith(item.suite + " > ") ? null : item.suite;
  const where = [suite, item.test, item.step].filter(Boolean).map(esc).join(" › ");
  const canAccept = ACCEPTABLE.includes(item.status);
  return '<section class="snap" aria-label="' + esc(item.step) + '">' +
    '<div class="snap-head">' + icon(status.cls, status.label) +
    '<div class="snap-title">' + where + ' <span class="badge">' + esc(item.kind) + '</span> <span class="badge ' + status.cls + '">' + esc(status.label) + "</span>" +
    '<div class="path muted">' + esc(item.baseline) + "</div></div>" +
    '<button data-accept="' + esc(item.id) + '"' + (canAccept && !locked() ? "" : " disabled") + ' title="' + (view.run.status === "running" ? "Wait for the run to finish" : "Copy the actual output over the baseline") + '">Accept</button></div>' +
    body(item) + "</section>";
}

function render() {
  document.getElementById("banners").innerHTML =
    (error ? '<div class="banner error">' + esc(error) + "</div>" : "") + (notice ? '<div class="banner ok">' + esc(notice) + "</div>" : "");
  document.getElementById("modes").innerHTML = compareModes(mode());
  if (!view) return;
  options(document.getElementById("run"), '<option value="">Latest run</option>' + view.runs.map((r) =>
    '<option value="' + esc(r.runId) + '">' + esc(r.runId) + " · " + esc(r.status) + "</option>").join(""), view.pinned ?? "");
  const pending = view.items.filter((item) => ACCEPTABLE.includes(item.status));
  const acceptAll = document.getElementById("accept-all");
  acceptAll.disabled = locked() || !pending.length;
  acceptAll.textContent = pending.length ? "Accept all changed (" + pending.length + ")" : "Accept all changed";

  const main = document.getElementById("main");
  const summary = document.getElementById("summary");
  if (!view.run) {
    summary.textContent = "";
    main.innerHTML = '<div class="empty muted">' + (view.pinned ? "Run " + esc(view.pinned) + " was not found." : 'No drill runs in <span class="path">.auditaur/drill-runs</span> yet.') + "</div>";
    return;
  }
  summary.innerHTML = esc(view.run.runId) + " · " + view.items.length + " snapshot result" + (view.items.length === 1 ? "" : "s") + " · " + pending.length + " to review";
  main.innerHTML = view.items.length
    ? view.items.map(card).join("")
    : '<div class="empty muted">Every snapshot in this run matched its baseline.</div>';
}

document.addEventListener("click", (event) => {
  const modeButton = event.target.closest("[data-compare-mode]");
  if (modeButton) {
    if (!view) return;
    view.ui.mode = modeButton.dataset.compareMode;
    render();
    return setView({ mode: view.ui.mode }).catch((err) => {
      error = err.message;
      load();
    });
  }
  const acceptButton = event.target.closest("[data-accept]");
  if (acceptButton) return accept([acceptButton.dataset.accept]);
  if (event.target.id === "accept-all") return accept(view.items.filter((item) => ACCEPTABLE.includes(item.status)).map((item) => item.id));
  if (event.target.id === "refresh") load();
});
document.getElementById("run").addEventListener("change", (event) => select({ runId: event.target.value || null }));
onServerChange(load);
load();
