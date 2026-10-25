const toggled = new Map();
let view = null;
let actionError = null;
let gateSig = null;
let timer = null;

async function post(path, body) {
  actionError = null;
  try {
    return await postJson(path, body);
  } catch (error) {
    actionError = error.message;
    return null;
  } finally {
    await load();
  }
}

async function load() {
  clearTimeout(timer);
  try {
    view = await getJson("/state");
  } catch (error) {
    view = { ...(view ?? { runs: [], gates: [], run: null }), loadError: "Canvas server unreachable: " + error.message };
  }
  render();
  timer = setTimeout(load, view.run?.status === "running" ? 500 : 3000);
}

function stepRow(step) {
  const duration = step.status === "running" ? since(step.startedAt) : step.durationMs;
  return '<div class="step">' + icon(step.status) +
    '<span class="phase muted">' + esc(step.phase === "test" ? "" : step.phase) + "</span>" +
    "<span>" + esc(step.title) + (step.soft ? ' <span class="tag">soft</span>' : "") + "</span>" +
    '<span class="duration muted">' + ms(duration) + "</span>" +
    (step.error ? '<div class="error">' + esc(step.error) + "</div>" : "") + "</div>";
}

function isOpen(id, auto) {
  return toggled.has(id) ? toggled.get(id) : auto;
}

function testRows(suite) {
  return suite.tests.map((test) => {
    const last = test.attempts.at(-1);
    const auto = test.result === "running" || test.result === "failed" || test.result === "interrupted";
    const open = isOpen(test.id, auto);
    const live = last?.status === "running" ? since(last?.steps?.[0]?.startedAt) : null;
    let html = '<div class="test" data-id="' + esc(test.id) + '" role="button" tabindex="0" aria-expanded="' + open + '">' +
      icon(test.result) + "<span>" + esc(test.name) + test.tags.map((tag) => '<span class="tag">@' + esc(tag) + "</span>").join("") +
      (test.attempts.length > 1 ? '<span class="tag">' + test.attempts.length + " attempts</span>" : "") + "</span>" +
      '<span class="duration muted">' + ms(live ?? test.durationMs) + "</span></div>";
    if (open) {
      html += '<div class="detail">' + (test.attempts.length ? test.attempts.map((attempt) =>
        (test.attempts.length > 1 ? '<div class="attempt-head ' + cls(attempt.status) + '">Attempt ' + (Number(attempt.attempt) + 1) + " · " + esc(attempt.status) + "</div>" : "") +
        (attempt.steps.length ? attempt.steps.map(stepRow).join("") : '<div class="muted">No steps yet.</div>') +
        (attempt.error && !attempt.steps.some((step) => step.error) ? '<div class="error">' + esc(attempt.error) + "</div>" : "")
      ).join("") : '<div class="muted">Not started.</div>') + "</div>";
    }
    return html;
  }).join("");
}

function gateCard(gate) {
  const choices = gate.choices.length ? gate.choices : [{ id: "", label: "Continue", outcome: "continue" }];
  const clip = gate.clipboard;
  return '<form class="gate" data-request="' + esc(gate.requestId) + '">' +
    "<h2>⏸ " + esc(gate.name) + "</h2>" +
    '<div class="muted" style="font-size:12px">' + esc(gate.test) + " · waiting since " + esc(new Date(gate.since).toLocaleTimeString()) +
    (gate.timeoutMs ? " · times out after " + ms(gate.timeoutMs) : "") + "</div>" +
    "<p>" + esc(gate.instructions) + "</p>" +
    (gate.selector ? '<p class="muted">Continues on its own when <span class="path">' + esc(gate.selector) + "</span>" +
      (gate.expectText ? " shows “" + esc(gate.expectText) + "”" : " appears") + ".</p>" : "") +
    (clip ? '<p class="muted">' + esc(clip.label) + ": " + (clip.value && !clip.sensitive && !clip.redacted ? '<span class="path">' + esc(clip.value) + "</span>" : "(hidden)") +
      " · clipboard " + esc(clip.status) + "</p>" : "") +
    gate.inputs.map((input) => "<label>" + esc(input.label ?? input.id) + (input.required ? " *" : "") +
      '<input name="' + esc(input.id) + '" type="' + (input.sensitive ? "password" : "text") + '" autocomplete="off"' + (input.required ? " required" : "") + "></label>").join("") +
    (gate.requestId
      ? '<div class="row">' + choices.map((choice, index) => '<button type="submit" value="' + esc(choice.id) + '" data-outcome="' + esc(choice.outcome) + '"' +
          (index === 0 ? ' class="primary"' : "") + ">" + esc(choice.label) + "</button>").join("") + "</div>"
      : '<p class="muted">This gate has no answer queue; answer it in the terminal running the drill.</p>') +
    "</form>";
}

function render() {
  if (!view) return;
  const pick = document.getElementById("pick");
  const options = '<option value="">Follow latest</option>' + view.runs.map((run) =>
    '<option value="' + esc(run.runId) + '">' + esc(run.runId) + " · " + esc(run.status) + "</option>").join("");
  if (pick.dataset.options !== options) {
    pick.innerHTML = options;
    pick.dataset.options = options;
  }
  if (document.activeElement !== pick) pick.value = view.pinned ?? "";

  const run = view.run;
  const banners = [];
  if (actionError) banners.push('<div class="banner error">' + esc(actionError) + "</div>");
  if (view.loadError) banners.push('<div class="banner error">' + esc(view.loadError) + "</div>");
  document.getElementById("banners").innerHTML = banners.join("");
  document.getElementById("cancel").hidden = run?.status !== "running";

  const sig = JSON.stringify(view.gates.map((gate) => gate.requestId + ":" + gate.since));
  if (sig !== gateSig) {
    gateSig = sig;
    document.getElementById("gates").innerHTML = view.gates.map(gateCard).join("");
    document.querySelector("#gates input")?.focus();
  }

  if (!run) {
    document.getElementById("status").innerHTML = "";
    document.getElementById("summary").textContent = "";
    document.getElementById("bar").hidden = true;
    document.getElementById("tree").innerHTML = '<div class="empty muted">' +
      (view.pinned ? "Run " + esc(view.pinned) + " was not found." : 'No drill runs in <span class="path">.auditaur/drill-runs</span> yet.') + "</div>";
    return;
  }
  const tests = run.suites.flatMap((suite) => suite.tests);
  const count = (result) => tests.filter((test) => test.result === result).length;
  const done = tests.filter((test) => !["running", "notRun"].includes(test.result)).length;
  document.getElementById("status").innerHTML = '<span class="' + cls(run.status) + '">' + esc(run.status) + "</span>";
  document.getElementById("summary").innerHTML = ['<span class="path">' + esc(run.runId) + "</span>", done + "/" + tests.length + " done"]
    .concat(["passed", "failed", "flaky", "skipped", "interrupted"].filter((result) => count(result))
      .map((result) => '<span class="' + result + '">' + count(result) + " " + result + "</span>"))
    .concat([ms(run.status === "running" ? since(run.startedAt) : run.durationMs)]).filter(Boolean).join(" · ");
  const bar = document.getElementById("bar");
  bar.hidden = !tests.length;
  bar.innerHTML = ["passed", "flaky", "failed", "interrupted", "skipped"].map((result) =>
    '<div style="width:' + (100 * count(result) / Math.max(1, tests.length)) + '%;background:currentColor" class="' + result + '"></div>').join("");

  document.getElementById("tree").innerHTML = run.suites.map((suite) => {
    const hooksOpen = isOpen("hooks:" + suite.id, suite.hooks.some((step) => step.status !== "passed"));
    return '<section class="suite"><div class="suite-head row">' + icon(suite.status === "passed" ? "passed" : suite.status) +
      '<span class="suite-name">' + esc(suite.suite) + '</span><span class="muted path">' + esc(suite.path) + "</span>" +
      '<span class="spacer"></span><span class="duration muted">' + ms(suite.durationMs) + "</span></div>" +
      (suite.error ? '<div class="error">' + esc(suite.error) + "</div>" : "") +
      (suite.hooks.length ? '<div class="test" data-id="' + esc("hooks:" + suite.id) + '" role="button" tabindex="0" aria-expanded="' + hooksOpen + '">' +
        icon(suite.hooks.some((step) => step.status === "running") ? "running" : suite.hooks.some((step) => ["failed", "timedOut"].includes(step.status)) ? "failed" : "passed") +
        '<span class="muted">Suite hooks</span><span></span></div>' +
        (hooksOpen ? '<div class="detail">' + suite.hooks.map(stepRow).join("") + "</div>" : "") : "") +
      testRows(suite) + "</section>";
  }).join("");
}

document.addEventListener("click", (event) => {
  const target = event.target.closest("button, .test[data-id]");
  if (!target || target.disabled) return;
  if (target.id === "cancel") return post("/cancel");
  if (target.classList.contains("test")) {
    const auto = target.getAttribute("aria-expanded") === "true";
    toggled.set(target.dataset.id, !auto);
    render();
  }
});
document.addEventListener("submit", async (event) => {
  event.preventDefault();
  const form = event.target;
  const button = event.submitter;
  const retry = button?.dataset.outcome === "retry";
  if (!retry && !form.reportValidity()) return;
  const inputs = [...form.querySelectorAll("input")].map((input) => ({ id: input.name, value: input.value }));
  form.querySelectorAll("button").forEach((each) => { each.disabled = true; });
  const answered = await post("/gate", { requestId: form.dataset.request, choiceId: button?.value || null, inputs });
  // Keep the card answerable when the answer was rejected; a retry answer re-arms the same gate.
  if (!answered || retry) {
    gateSig = null;
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
document.getElementById("pick").addEventListener("change", (event) => post("/follow", { runId: event.target.value || null }));
load();
