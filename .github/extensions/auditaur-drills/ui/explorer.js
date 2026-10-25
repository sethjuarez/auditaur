const LABELS = { passed: "passed", failed: "failed", flaky: "flaky", interrupted: "interrupted", running: "running", skipped: "skipped", notRun: "not run" };
const expanded = new Set();
let view = null;
let busy = false;
let actionError = null;
let timer = null;

const running = () => Boolean(view?.active) || Boolean(view?.launch && !view.launch.exited);

async function post(path, body) {
  busy = true;
  actionError = null;
  render();
  try {
    await postJson(path, body);
  } catch (error) {
    actionError = error.message;
  } finally {
    busy = false;
    await load();
  }
}

async function load(refresh) {
  clearTimeout(timer);
  try {
    view = await getJson("/state" + (refresh ? "?refresh" : ""));
  } catch (error) {
    view = { ...(view ?? { suites: [], runs: [], invalid: [] }), listError: "Canvas server unreachable: " + error.message };
  }
  render();
  timer = setTimeout(load, running() ? 1000 : 5000);
}

function matches(test, query) {
  if (!query) return true;
  const haystack = (test.title + " " + test.tags.map((tag) => "@" + tag).join(" ")).toLowerCase();
  return query.toLowerCase().split(/\s+/).every((word) => haystack.includes(word));
}

function testRow(test, disabled) {
  const result = test.last?.result ?? "notRun";
  const open = expanded.has(test.id);
  const mode = test.mode && test.mode !== "run"
    ? '<span class="badge mode" title="' + esc(test.modeReason) + '">' + esc(test.mode) + "</span>" : "";
  const dots = test.history.map((entry) =>
    '<span class="dot ' + entry.result + '" title="' + esc(entry.runId + ": " + LABELS[entry.result]) + '"></span>').join("");
  let html = '<div class="test" data-id="' + esc(test.id) + '" role="button" tabindex="0" aria-expanded="' + open + '">' +
    icon(result, LABELS[result]) +
    "<span>" + esc(test.name) + test.tags.map((tag) => '<span class="tag">@' + esc(tag) + "</span>").join("") +
    (test.flaky ? '<span class="badge flaky">flaky</span>' : "") +
    (test.slow ? '<span class="badge">slow</span>' : "") + mode + "</span>" +
    '<span class="history" aria-label="Recent results, oldest first">' + dots + "</span>" +
    '<span class="duration muted">' + ms(test.last?.durationMs) + "</span>" +
    '<button data-run-test="' + esc(test.id) + '"' + (disabled ? " disabled" : "") + ' aria-label="Run ' + esc(test.title) + '">Run</button></div>';
  if (open) {
    html += '<div class="detail">' +
      (test.last?.error ? '<div class="error">' + esc(test.last.error) + "</div>" : "") +
      "<ol>" + test.steps.map((step) => "<li>" + esc(step.title) + "</li>").join("") + "</ol>" +
      '<span class="muted path">' + esc(test.title) + " · " + esc(test.id) + "</span></div>";
  }
  return html;
}

function render() {
  if (!view) return;
  const query = document.getElementById("filter").value.trim();
  const tests = view.suites.flatMap((suite) => suite.tests);
  document.getElementById("counts").textContent =
    view.suites.length + (view.suites.length === 1 ? " suite · " : " suites · ") + tests.length + (tests.length === 1 ? " test" : " tests");
  const last = view.runs.find((run) => run.status !== "running");
  const lastClass = last?.status === "passed" ? "passed" : last?.status === "interrupted" ? "interrupted" : "failed";
  document.getElementById("last-run").innerHTML = last
    ? 'Last run <span class="' + lastClass + '">' + esc(last.status) + "</span> · " +
      Number(last.totals?.passed) + "/" + Number(last.totals?.tests) + " passed" + (last.totals?.flaky ? " · " + Number(last.totals.flaky) + " flaky" : "") +
      " · " + ms(last.durationMs) + ' · <span class="path">' + esc(last.runId) + "</span>"
    : "No runs yet";

  const banners = [];
  if (view.active) {
    const pct = view.active.total ? Math.round(100 * view.active.done / view.active.total) : 0;
    banners.push('<div class="banner run">Running ' + view.active.done + "/" + view.active.total +
      (view.active.current ? " · " + esc(view.active.current) : "") + '<div class="bar"><div style="width:' + pct + '%"></div></div></div>');
  } else if (view.launch && !view.launch.exited) {
    banners.push('<div class="banner run">Starting drill test…</div>');
  }
  if (actionError) banners.push('<div class="banner error">' + esc(actionError) + "</div>");
  if (view.launch?.exited && view.launch.output) {
    banners.push('<div class="banner error">' + (view.launch.exitCode == null ? "drill test failed to start" : "drill test exited with code " + esc(view.launch.exitCode)) + ":\n" + esc(view.launch.output) + "</div>");
  }
  if (view.listError) banners.push('<div class="banner error">' + esc(view.listError) + "</div>");
  for (const invalid of view.invalid) {
    const messages = (invalid.errors ?? []).map((error) => error.message ?? JSON.stringify(error)).join("; ");
    banners.push('<div class="banner error"><span class="path">' + esc(invalid.path) + "</span>: " + esc(messages) + "</div>");
  }
  document.getElementById("banners").innerHTML = banners.join("");

  const disabled = running() || busy;
  document.getElementById("run-all").disabled = disabled || !tests.length;
  document.getElementById("run-failed").disabled = disabled || !last;
  document.getElementById("cancel").hidden = !view.active;

  const sections = view.suites.map((suite) => {
    const shown = suite.tests.filter((test) => matches(test, query));
    if (!shown.length) return "";
    const failing = suite.tests.filter((test) => test.last?.result === "failed").length;
    return '<section class="suite"><div class="suite-head row">' +
      '<span class="suite-name">' + esc(suite.suite) + "</span>" +
      suite.tags.map((tag) => '<span class="tag">@' + esc(tag) + "</span>").join("") +
      '<span class="muted path">' + esc(suite.path) + '</span><span class="spacer"></span>' +
      (failing ? '<span class="failed" style="font-size:12px">' + failing + " failing</span>" : "") +
      '<button data-run-suite="' + esc(suite.suite) + '"' + (disabled ? " disabled" : "") + ">Run suite</button></div>" +
      shown.map((test) => testRow(test, disabled)).join("") + "</section>";
  }).join("");
  document.getElementById("main").innerHTML = sections ||
    '<div class="empty muted">' + (view.suites.length ? "No tests match the filter." : 'No drill tests in <span class="path">.auditaur/drills</span>.') + "</div>";
}

document.addEventListener("click", (event) => {
  const target = event.target.closest("button, .test[data-id]");
  if (!target || target.disabled) return;
  if (target.dataset.runTest) return post("/run", { test: target.dataset.runTest });
  if (target.dataset.runSuite) return post("/run", { suite: target.dataset.runSuite });
  if (target.id === "run-all") return post("/run", {});
  if (target.id === "run-failed") return post("/run", { lastFailed: true });
  if (target.id === "cancel") return post("/cancel");
  if (target.id === "refresh") return load(true);
  if (target.dataset.id) {
    if (expanded.has(target.dataset.id)) expanded.delete(target.dataset.id);
    else expanded.add(target.dataset.id);
    render();
  }
});
document.addEventListener("keydown", (event) => {
  const row = event.target.closest?.(".test[data-id]");
  if (row && event.target === row && (event.key === "Enter" || event.key === " ")) {
    event.preventDefault();
    row.click();
  }
});
document.getElementById("filter").addEventListener("input", render);
load();
