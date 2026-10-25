const VIEWS = [["commands", "Commands"], ["events", "Events"], ["tests", "Tests"], ["map", "Map"]];
let view = null;
let error = null;
let current = "commands";
let missedOnly = false;
let timer = null;

const titles = () => new Map((view.coverage?.tests ?? []).map((test) => [test.id, test.title]));
const plural = (count, word) => count + " " + word + (count === 1 ? "" : "s");

async function load(refresh = false) {
  clearTimeout(timer);
  try {
    view = await getJson(refresh ? "/state?refresh" : "/state");
    error = view.error;
  } catch (err) {
    error = "Canvas server unreachable: " + err.message;
  }
  render();
  if (view?.run?.status === "running") timer = setTimeout(load, 2000);
}

async function post(path, body) {
  try {
    await postJson(path, body);
    error = null;
  } catch (err) {
    error = err.message;
  }
  await load();
}

// One command or event: whether any test exercised it, counts, and the tests that did.
function itemRow(name, stats, testIds, names) {
  const hit = testIds.length > 0;
  const tests = hit
    ? testIds.map((id) => '<span class="tag">' + esc(names.get(id) ?? id) + "</span>").join("")
    : '<span class="muted">No test exercised this.</span>';
  return '<div class="item' + (hit ? "" : " missed") + '">' +
    icon(hit ? "passed" : "notRun", hit ? "Exercised" : "Not exercised") +
    '<div><div class="item-name">' + esc(name) + '</div><div class="item-tests">' + tests + "</div></div>" +
    '<div class="item-stats muted">' + stats.filter(Boolean).join(" · ") + "</div></div>";
}

function seen(item) {
  return view.dbs.length ? plural(item.seen, "record") + " in app" : null;
}

function commandRows(names) {
  return view.coverage.commands.filter((item) => !missedOnly || !item.tests.length).map((item) =>
    itemRow(item.command, [
      plural(item.calls, "call"),
      item.failed ? '<span class="failed">' + item.failed + " failed</span>" : null,
      seen(item),
    ], item.tests, names));
}

function eventRows(names) {
  return view.coverage.events.filter((item) => !missedOnly || !item.tests.length).map((item) =>
    itemRow(item.event, [plural(item.count, "record"), esc([...item.directions].join("/")), seen(item)], item.tests, names));
}

function testRows() {
  return view.coverage.tests
    .filter((test) => !missedOnly || (!test.commands.length && !test.events.length))
    .map((test) => {
      const touched = [...test.commands, ...test.events];
      const tags = touched.length ? touched.map((name) => '<span class="tag mono">' + esc(name) + "</span>").join("") : '<span class="muted">No IPC or events.</span>';
      const note = {
        none: "No telemetry recorded.",
        unreadable: "Telemetry unreadable: " + esc(test.error),
        partial: "Some attempts' telemetry is unreadable: " + esc(test.error),
      }[test.telemetry];
      const detail = (test.telemetry === "recorded" || test.telemetry === "partial" ? tags : "") +
        (note ? '<div class="muted">' + note + "</div>" : "");
      return '<div class="item">' + icon(test.status, test.status) +
        '<div><div>' + esc(test.title) + '</div><div class="item-tests">' + detail + "</div></div>" +
        '<div class="item-stats muted">' + plural(test.commands.length, "command") + " · " + plural(test.events.length, "event") + "</div></div>";
    });
}

function mapView() {
  const coverage = view.coverage;
  const cols = [
    ...coverage.commands.map((item) => ({ key: "c:" + item.command, label: item.command, cls: item.tests.length ? "" : "muted", title: "command " + item.command })),
    ...coverage.events.map((item) => ({ key: "e:" + item.event, label: item.event, cls: item.tests.length ? "" : "muted", title: "event " + item.event })),
  ].filter((col) => !missedOnly || col.cls);
  const rows = coverage.tests.map((test) => ({
    key: test.id,
    label: test.title,
    touched: new Set([...test.commands.map((name) => "c:" + name), ...test.events.map((name) => "e:" + name)]),
  }));
  return matrix({
    rows,
    cols,
    corner: "test \\ command, event",
    cell: (row, col) => (row.touched.has(col.key) ? { cls: "passed", title: row.label + " → " + col.title } : null),
  });
}

const EMPTY = {
  commands: "No IPC commands were recorded. Compare with app telemetry to list commands no test called.",
  events: "No events were recorded. Compare with app telemetry to list events no test touched.",
  tests: "No tests in this run.",
};

function renderMain() {
  const main = document.getElementById("main");
  if (!view.run) {
    main.innerHTML = '<div class="empty muted">' + (view.pinned ? "Run " + esc(view.pinned) + " was not found." : 'No drill runs in <span class="path">.auditaur/drill-runs</span> yet.') + "</div>";
    return;
  }
  if (!view.coverage) {
    main.innerHTML = '<div class="empty muted">Coverage is unavailable for this run.</div>';
    return;
  }
  if (current === "map") {
    main.innerHTML = mapView();
    return;
  }
  const names = titles();
  const rows = current === "commands" ? commandRows(names) : current === "events" ? eventRows(names) : testRows();
  main.innerHTML = rows.length ? rows.join("") : '<div class="empty muted">' + (missedOnly ? "Nothing unexercised here." : EMPTY[current]) + "</div>";
}

function renderMeters() {
  const totals = view.coverage?.totals;
  document.getElementById("meters").innerHTML = totals
    ? meter("Commands exercised", totals.commandsExercised, totals.commands) +
      meter("Events exercised", totals.eventsExercised, totals.events) +
      meter("Tests with telemetry", totals.testsWithTelemetry, totals.tests)
    : "";
}

function render() {
  const sources = (view?.coverage?.sources ?? []).filter((source) => source.error);
  document.getElementById("banners").innerHTML =
    (error ? '<div class="banner error">' + esc(error) + "</div>" : "") +
    sources.map((source) => '<div class="banner error">' + esc(source.db) + ": " + esc(source.error) + "</div>").join("");
  document.getElementById("views").innerHTML = segmented("View", VIEWS, current);
  if (!view) return;
  options(document.getElementById("run"), '<option value="">Latest run</option>' + view.runs.map((r) =>
    '<option value="' + esc(r.runId) + '">' + esc(r.runId) + " · " + esc(r.status) + "</option>").join(""), view.pinned ?? "");
  document.querySelector("#stores summary").textContent = "Compare with app telemetry" + (view.dbs.length ? " (" + view.dbs.length + ")" : "");
  renderMeters();
  renderMain();
}

// The store picker: discovered app stores plus any already chosen, narrowed by the filter.
const picker = { stores: [], chosen: new Set(), error: null };

function renderStores() {
  const query = document.getElementById("store-filter").value;
  document.getElementById("store-error").innerHTML = picker.error ? '<div class="error">' + esc(picker.error) + "</div>" : "";
  document.getElementById("store-rows").innerHTML = picker.stores.length
    ? checklist(picker.stores, picker.chosen, query)
    : '<span class="muted">No Auditaur app sessions found. Run your app with Auditaur enabled.</span>';
}

async function loadStores() {
  picker.chosen = new Set(view.dbs);
  let stores = [];
  try {
    const result = await getJson("/apps");
    stores = result.stores;
    picker.error = result.error;
  } catch (err) {
    picker.error = err.message;
  }
  const known = new Set(stores.map((store) => store.db));
  const extra = view.dbs.filter((db) => !known.has(db)).map((db) => ({ db, serviceName: "chosen store" }));
  picker.stores = [...stores, ...extra].map((store) => ({
    value: store.db,
    label: store.serviceName ?? "app",
    detail: store.sessionId ? store.sessionId + (store.sessions > 1 ? " +" + (store.sessions - 1) : "") : store.db,
    badge: store.status,
  }));
  renderStores();
}

document.addEventListener("click", (event) => {
  const segment = event.target.closest("[data-segment]");
  if (segment) {
    current = segment.dataset.segment;
    return render();
  }
  if (event.target.id === "apply-stores") return post("/sources", { dbs: [...picker.chosen] });
  if (event.target.id === "refresh") load(true);
});
document.getElementById("store-rows").addEventListener("change", (event) => {
  const value = event.target.dataset.check;
  if (value == null) return;
  if (event.target.checked) picker.chosen.add(value);
  else picker.chosen.delete(value);
  renderStores();
});
document.getElementById("store-filter").addEventListener("input", renderStores);
document.getElementById("missed").addEventListener("change", (event) => {
  missedOnly = event.target.checked;
  render();
});
document.getElementById("stores").addEventListener("toggle", (event) => {
  if (event.target.open && view) loadStores();
});
document.getElementById("run").addEventListener("change", (event) => post("/select", { runId: event.target.value || null }));
load();
