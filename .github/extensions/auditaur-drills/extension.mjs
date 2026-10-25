// Extension: auditaur-drills
// Canvases over Auditaur drill tests. They read `auditaur drill ... --json` output and the
// versioned run folders under `.auditaur/drill-runs/`; they never compute results themselves.

import { createServer } from "node:http";
import { execFile, spawn } from "node:child_process";
import { closeSync, openSync, readFileSync } from "node:fs";
import { readdir, readFile, stat, unlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { joinSession, createCanvas, CanvasError } from "@github/copilot-sdk/extension";

// `.github/extensions/auditaur-drills/extension.mjs` -> repository root.
const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
const RUNS_DIR = join(".auditaur", "drill-runs");
const DRILLS_DIR = join(".auditaur", "drills");
const CANCEL_REQUEST_FILE = "cancel.request";
const HISTORY_RUNS = 10;
const LIST_TIMEOUT_MS = 60_000;

const servers = new Map();
// Workspaces with a launch between its checks and the spawned run's folder appearing.
const launching = new Set();
const LOG_PREFIX = "auditaur-drills-canvas-";

// Logs are deleted when a launched run exits, unless the extension that launched it died first.
async function sweepOrphanLogs() {
    let names;
    try {
        names = await readdir(tmpdir());
    } catch {
        return;
    }
    for (const name of names) {
        const owner = Number(name.match(/^auditaur-drills-canvas-(\d+)-\d+\.log$/)?.[1]);
        if (owner && owner !== process.pid && !processAlive(owner)) {
            // A run still writing holds the file open on Windows; it is retried next startup.
            await unlink(join(tmpdir(), name)).catch(() => {});
        }
    }
}

const openSchema = {
    type: "object",
    additionalProperties: false,
    properties: {
        cwd: {
            type: "string",
            description: "Folder containing .auditaur/drills, relative to the repository root. Defaults to the root.",
        },
        cli: {
            type: "string",
            description: "Path to the auditaur CLI. Defaults to $AUDITAUR_BIN, then `auditaur` on PATH.",
        },
        run: {
            type: "object",
            additionalProperties: false,
            description: "How the canvas's run buttons launch `drill test`.",
            properties: {
                app: { type: "string", description: "--app name." },
                command: {
                    type: "array",
                    items: { type: "string" },
                    description: "App launch command passed after `--`. Omit to use the config `start` command.",
                },
                args: {
                    type: "array",
                    items: { type: "string" },
                    description: "Extra drill test arguments, e.g. [\"--grep-invert=@visual\"].",
                },
            },
        },
    },
};

const runSchema = {
    type: "object",
    additionalProperties: false,
    properties: {
        test: { type: "string", description: "Run one test by its `suite > test` title or id." },
        suite: { type: "string", description: "Run one suite by name." },
        grep: { type: "string", description: "Run tests whose `suite > test @tags` match this regex." },
        lastFailed: { type: "boolean", description: "Rerun the tests that failed or did not run last time." },
    },
};

function escapeRegex(value) {
    return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function workspaceFor(input) {
    const cwd = input?.cwd ?? ".";
    return isAbsolute(cwd) ? cwd : resolve(REPO_ROOT, cwd);
}

function cliFor(input) {
    const cli = input?.cli ?? process.env.AUDITAUR_BIN ?? "auditaur";
    // A bare command name is looked up on PATH; anything with a separator is a path.
    if (/[\\/]/.test(cli) && !isAbsolute(cli)) {
        return resolve(REPO_ROOT, cli);
    }
    return cli;
}

function cliJson(state, args) {
    return new Promise((resolvePromise) => {
        execFile(
            state.cli,
            [...args, "--json"],
            { cwd: state.workspace, timeout: LIST_TIMEOUT_MS, windowsHide: true, maxBuffer: 32 * 1024 * 1024 },
            (error, stdout, stderr) => {
                try {
                    resolvePromise({ value: JSON.parse(stdout) });
                } catch {
                    const detail = (stderr || stdout || error?.message || "no output").trim();
                    resolvePromise({ error: `\`${state.cli} ${args.join(" ")}\` failed: ${detail.slice(-2000)}` });
                }
            },
        );
    });
}

async function walkSignature(dir) {
    let signature = "";
    let entries;
    try {
        entries = await readdir(dir, { withFileTypes: true });
    } catch {
        return "missing";
    }
    for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
        const path = join(dir, entry.name);
        if (entry.isDirectory()) {
            if (entry.name !== "__snapshots__") {
                signature += `${entry.name}/{${await walkSignature(path)}}`;
            }
        } else if (entry.name.endsWith(".json")) {
            const info = await stat(path).catch(() => null);
            signature += `${entry.name}:${info?.mtimeMs}:${info?.size};`;
        }
    }
    return signature;
}

// `drill list` is re-run only when a drill file changes or a refresh is forced.
async function loadSuites(state, force = false) {
    const signature = await walkSignature(join(state.workspace, DRILLS_DIR));
    if (!force && state.list && state.listSignature === signature) {
        return;
    }
    const { value, error } = await cliJson(state, ["drill", "list"]);
    state.listSignature = signature;
    state.list = value ?? null;
    state.listError = value ? null : error;
}

function processAlive(pid) {
    try {
        process.kill(pid, 0);
        return true;
    } catch (error) {
        return error.code === "EPERM";
    }
}

async function readRuns(state) {
    const root = join(state.workspace, RUNS_DIR);
    let ids;
    try {
        ids = (await readdir(root, { withFileTypes: true }))
            .filter((entry) => entry.isDirectory() && /^\d{8}-\d{6}-\d{3}/.test(entry.name))
            .map((entry) => entry.name)
            .sort()
            .reverse()
            .slice(0, HISTORY_RUNS);
    } catch {
        return [];
    }
    const runs = [];
    for (const id of ids) {
        try {
            const report = JSON.parse(await readFile(join(root, id, "run.json"), "utf8"));
            // Same rule as `drill show`: a running run whose owner died was interrupted.
            if (report.status === "running" && !processAlive(report.pid)) {
                report.status = "interrupted";
                for (const suite of report.suites ?? []) {
                    for (const test of suite.tests ?? []) {
                        if (test.status === "running") test.status = "interrupted";
                    }
                }
            }
            runs.push(report);
        } catch {
            // Being written or pruned; the next poll picks it up.
        }
    }
    return runs;
}

// One test's result in one run, folded to what the explorer shows.
function resultOf(test) {
    if (!test || test.status === "pending") return "notRun";
    if (test.status === "running") return "running";
    if (test.outcome === "flaky") return "flaky";
    if (test.status === "skipped") return "skipped";
    if (test.status === "interrupted") return "interrupted";
    if (test.outcome === "expected") return "passed";
    return "failed";
}

function buildView(state, runs) {
    const byRun = runs.map((run) => {
        const tests = new Map();
        for (const suite of run.suites ?? []) {
            for (const test of suite.tests ?? []) {
                tests.set(test.id, {
                    result: resultOf(test),
                    durationMs: test.durationMs,
                    error: test.attempts?.at(-1)?.error?.message ?? null,
                });
            }
        }
        return { runId: run.runId, tests };
    });
    const suites = (state.list?.suites ?? []).map((suite) => ({
        suite: suite.suite,
        path: suite.path,
        description: suite.description,
        tags: suite.tags ?? [],
        tests: (suite.tests ?? []).map((test) => {
            const history = byRun
                .filter((run) => run.tests.has(test.id))
                .map((run) => ({ runId: run.runId, ...run.tests.get(test.id) }));
            const last = history.find((entry) => entry.result !== "notRun") ?? null;
            return {
                id: test.id,
                name: test.name,
                title: test.title,
                tags: test.tags ?? [],
                mode: test.mode,
                modeReason: test.modeReason,
                slow: test.slow,
                steps: test.steps ?? [],
                last,
                // Oldest first, so the newest result sits at the right edge.
                history: history.slice().reverse(),
                flaky: history.some((entry) => entry.result === "flaky"),
            };
        }),
    }));
    const active = runs.find((run) => run.status === "running") ?? null;
    let progress = null;
    if (active) {
        const tests = (active.suites ?? []).flatMap((suite) => suite.tests ?? []);
        progress = {
            runId: active.runId,
            done: tests.filter((test) => !["pending", "running"].includes(test.status)).length,
            total: tests.length,
            current: tests.find((test) => test.status === "running")?.title ?? null,
        };
    }
    return {
        workspace: state.workspace,
        cli: state.cli,
        listError: state.listError,
        invalid: state.list?.invalid ?? [],
        suites,
        runs: runs.map((run) => ({
            runId: run.runId,
            status: run.status,
            totals: run.totals,
            durationMs: run.durationMs,
            startedAt: run.startedAt,
        })),
        active: progress,
        launch: state.launch,
    };
}

async function snapshot(state, force = false) {
    await loadSuites(state, force);
    return buildView(state, await readRuns(state));
}

function runArgs(state, request) {
    const args = ["drill", "test"];
    if (request.test) {
        const tests = (state.list?.suites ?? []).flatMap((suite) => suite.tests ?? []);
        const match = tests.find((test) => test.id === request.test || test.title === request.test);
        if (!match) {
            throw new CanvasError("unknown_test", `no drill test is titled or identified \`${request.test}\``);
        }
        // The grep haystack is `suite > test @tag...`.
        args.push(`--grep=^${escapeRegex(match.title)}( @|$)`);
    } else if (request.suite) {
        args.push(`--grep=^${escapeRegex(request.suite)} > `);
    } else if (request.grep) {
        args.push(`--grep=${request.grep}`);
    }
    if (request.lastFailed) {
        args.push("--last-failed");
    }
    const run = state.run ?? {};
    if (run.app) {
        args.push(`--app=${run.app}`);
    }
    args.push(...(run.args ?? []));
    if (run.command?.length) {
        args.push("--", ...run.command);
    }
    return args;
}

async function startRun(state, request) {
    // Claimed synchronously so concurrent requests, from this panel or another one on the same
    // workspace, cannot both pass the checks below.
    if (launching.has(state.workspace) || (state.launch && !state.launch.exited)) {
        throw new CanvasError("run_in_progress", "a run started from this canvas is still going");
    }
    const workspace = state.workspace;
    launching.add(workspace);
    state.starting = true;
    try {
        return await launchRun(state, request);
    } finally {
        state.starting = false;
        launching.delete(workspace);
    }
}

async function launchRun(state, request) {
    const runs = await readRuns(state);
    if (runs.some((run) => run.status === "running")) {
        throw new CanvasError("run_in_progress", "another drill run is in progress; cancel it first");
    }
    await loadSuites(state);
    const args = runArgs(state, request);
    const log = join(tmpdir(), `${LOG_PREFIX}${process.pid}-${Date.now()}.log`);
    const fd = openSync(log, "w");
    let child;
    try {
        child = spawn(state.cli, args, {
            cwd: state.workspace,
            detached: true,
            windowsHide: true,
            stdio: ["ignore", fd, fd],
        });
    } finally {
        closeSync(fd);
    }
    const launch = {
        pid: child.pid ?? null,
        workspace: state.workspace,
        args,
        startedAt: new Date().toISOString(),
        exited: false,
        exitCode: null,
        output: null,
    };
    state.launch = launch;
    const finish = (code, message) => {
        if (launch.exited) return;
        launch.exited = true;
        launch.exitCode = code;
        // 0 passed, 1 failed, 130 cancelled: the run folder tells the story. Anything else is a launch problem.
        if (message || ![0, 1, 130].includes(code)) {
            let tail = "";
            try {
                tail = readFileSync(log, "utf8").slice(-4000).trim();
            } catch {
                // Nothing captured.
            }
            launch.output = [message, tail].filter(Boolean).join("\n") || `exit code ${code}`;
        }
        unlink(log).catch(() => {});
    };
    child.on("error", (error) => finish(null, String(error?.message ?? error)));
    child.on("exit", (code) => finish(code, null));
    child.unref();
    return { started: true, pid: launch.pid, args };
}

async function cancelRun(state) {
    const running = (await readRuns(state)).filter((run) => run.status === "running");
    // Prefer the run this canvas launched; otherwise the newest run in the workspace.
    const ours = state.launch && !state.launch.exited ? state.launch.pid : null;
    const active = running.find((run) => run.pid === ours) ?? running[0];
    if (!active) {
        throw new CanvasError("no_active_run", "no drill run is in progress");
    }
    await writeFile(join(state.workspace, RUNS_DIR, active.runId, CANCEL_REQUEST_FILE), "");
    return { runId: active.runId, cancelRequested: true };
}

function summary(view) {
    const tests = view.suites.flatMap((suite) => suite.tests);
    return {
        workspace: view.workspace,
        listError: view.listError,
        invalidFiles: view.invalid.length,
        suites: view.suites.length,
        tests: tests.length,
        lastRun: view.runs.find((run) => run.status !== "running") ?? null,
        active: view.active,
        failing: tests.filter((test) => test.last?.result === "failed").map((test) => test.title),
        flaky: tests.filter((test) => test.flaky).map((test) => test.title),
        neverRun: tests.filter((test) => !test.last).map((test) => test.title),
    };
}

function jsonResponse(res, value, status = 200) {
    res.writeHead(status, { "Content-Type": "application/json; charset=utf-8", "Cache-Control": "no-store" });
    res.end(JSON.stringify(value));
}

async function readBody(req) {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    const text = Buffer.concat(chunks).toString("utf8");
    return text ? JSON.parse(text) : {};
}

async function startServer(state) {
    const server = createServer(async (req, res) => {
        try {
            const url = new URL(req.url ?? "/", "http://127.0.0.1");
            if (req.method === "GET" && url.pathname === "/state") {
                jsonResponse(res, await snapshot(state, url.searchParams.has("refresh")));
            } else if (req.method === "POST" && url.pathname === "/run") {
                jsonResponse(res, await startRun(state, await readBody(req)));
            } else if (req.method === "POST" && url.pathname === "/cancel") {
                jsonResponse(res, await cancelRun(state));
            } else if (req.method === "GET" && url.pathname === "/") {
                res.writeHead(200, { "Content-Type": "text/html; charset=utf-8", "Cache-Control": "no-store" });
                res.end(EXPLORER_HTML);
            } else {
                jsonResponse(res, { error: "not found" }, 404);
            }
        } catch (error) {
            jsonResponse(res, { error: String(error?.message ?? error) }, error instanceof CanvasError ? 409 : 500);
        }
    });
    await new Promise((resolvePromise) => server.listen(0, "127.0.0.1", resolvePromise));
    return { server, url: `http://127.0.0.1:${server.address().port}/` };
}

function configure(state, input) {
    state.workspace = workspaceFor(input);
    state.cli = cliFor(input);
    state.run = input?.run ?? {};
    state.list = null;
    state.listSignature = null;
    state.listError = null;
    return state;
}

function entryFor(ctx) {
    const entry = servers.get(ctx.instanceId);
    if (!entry) {
        throw new CanvasError("not_open", "this canvas instance is not open");
    }
    return entry;
}

const EXPLORER_HTML = String.raw`<!doctype html>
<html>
<head>
<meta charset="utf-8">
<title>Drill tests</title>
<style>
  body {
    margin: 0;
    background: var(--background-color-default, #fff);
    color: var(--text-color-default, #1f2328);
    font-family: var(--font-sans, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif);
    font-size: var(--text-body-medium, 14px);
    line-height: var(--leading-body-medium, 20px);
  }
  header { position: sticky; top: 0; z-index: 1; background: var(--background-color-default, #fff); padding: 12px 16px 8px; border-bottom: 1px solid var(--border-color-default, #d0d7de); }
  h1 { margin: 0; font-size: 16px; font-weight: var(--font-weight-semibold, 600); }
  .muted { color: var(--text-color-muted, #656d76); }
  .row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .spacer { flex: 1; }
  button {
    font: inherit; font-size: 12px; padding: 2px 10px; border-radius: 6px; cursor: pointer;
    border: 1px solid var(--border-color-default, #d0d7de); background: transparent; color: inherit;
  }
  button:hover:not(:disabled) { border-color: var(--text-color-muted, #656d76); }
  button:disabled { opacity: .5; cursor: default; }
  button:focus-visible, input:focus-visible, .test:focus-visible { outline: 2px solid var(--color-focus-outline, #0969da); outline-offset: 1px; }
  input[type=search] {
    font: inherit; font-size: 12px; padding: 3px 8px; border-radius: 6px; min-width: 180px; flex: 1;
    border: 1px solid var(--border-color-default, #d0d7de); background: transparent; color: inherit;
  }
  main { padding: 0 16px 24px; }
  .banner { margin-top: 8px; padding: 6px 10px; border-radius: 6px; font-size: 12px; white-space: pre-wrap; }
  .banner.run { background: var(--true-color-blue-muted, #ddf4ff); }
  .banner.error { background: var(--true-color-red-muted, #ffebe9); }
  .bar { height: 4px; border-radius: 2px; background: var(--border-color-default, #d0d7de); margin-top: 4px; overflow: hidden; }
  .bar > div { height: 100%; background: var(--true-color-blue, #0969da); transition: width .3s; }
  .suite { margin-top: 12px; }
  .suite-head { padding: 6px 0; border-bottom: 1px solid var(--border-color-default, #d0d7de); }
  .suite-name { font-weight: var(--font-weight-semibold, 600); }
  .path { font-family: var(--font-mono, Consolas, monospace); font-size: 11px; }
  .test { display: grid; grid-template-columns: 18px 1fr auto auto auto; gap: 8px; align-items: center; padding: 4px 0 4px 4px; border-bottom: 1px solid var(--border-color-default, #d0d7de); cursor: pointer; }
  .test:hover { background: color-mix(in srgb, var(--text-color-muted, #656d76) 8%, transparent); }
  .icon { text-align: center; font-weight: 700; }
  .tag, .badge { font-size: 11px; padding: 0 6px; border-radius: 10px; border: 1px solid var(--border-color-default, #d0d7de); margin-left: 4px; white-space: nowrap; }
  .badge.flaky { border-color: var(--true-color-orange, #bc4c00); }
  .badge.mode { border-style: dashed; }
  .history { display: flex; gap: 2px; }
  .dot { width: 8px; height: 8px; border-radius: 2px; background: var(--border-color-default, #d0d7de); }
  .duration { font-size: 11px; min-width: 48px; text-align: right; }
  .passed { color: var(--true-color-green, #1a7f37); } .dot.passed { background: var(--true-color-green, #1a7f37); }
  .failed { color: var(--true-color-red, #cf222e); } .dot.failed { background: var(--true-color-red, #cf222e); }
  .flaky { color: var(--true-color-orange, #bc4c00); } .dot.flaky { background: var(--true-color-orange, #bc4c00); }
  .interrupted { color: var(--true-color-yellow, #9a6700); } .dot.interrupted { background: var(--true-color-yellow, #9a6700); }
  .running { color: var(--true-color-blue, #0969da); } .dot.running { background: var(--true-color-blue, #0969da); }
  .skipped, .notRun { color: var(--text-color-muted, #656d76); }
  .detail { font-size: 12px; padding: 2px 0 8px 30px; border-bottom: 1px solid var(--border-color-default, #d0d7de); }
  .detail ol { margin: 4px 0; padding-left: 20px; }
  .detail .error { color: var(--true-color-red, #cf222e); white-space: pre-wrap; font-family: var(--font-mono, Consolas, monospace); font-size: 11px; }
  .empty { padding: 24px 0; text-align: center; }
</style>
</head>
<body>
<header>
  <div class="row">
    <h1>Drill tests</h1>
    <span id="counts" class="muted"></span>
    <span class="spacer"></span>
    <button id="run-all">Run all</button>
    <button id="run-failed">Rerun failed</button>
    <button id="cancel" hidden>Cancel run</button>
    <button id="refresh" title="Reload drill files">Refresh</button>
  </div>
  <div class="row" style="margin-top:8px">
    <input id="filter" type="search" placeholder="Filter by title or @tag" aria-label="Filter tests">
    <span id="last-run" class="muted" style="font-size:12px"></span>
  </div>
  <div id="banners" aria-live="polite"></div>
</header>
<main id="main"><div class="empty muted">Loading…</div></main>
<script>
const ICONS = { passed: "✓", failed: "✗", flaky: "◐", interrupted: "!", running: "●", skipped: "⊘", notRun: "·" };
const LABELS = { passed: "passed", failed: "failed", flaky: "flaky", interrupted: "interrupted", running: "running", skipped: "skipped", notRun: "not run" };
const expanded = new Set();
let view = null;
let busy = false;
let actionError = null;
let timer = null;

const esc = (value) => String(value ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const ms = (raw) => { const value = Number(raw); return raw == null || Number.isNaN(value) ? "" : value < 1000 ? value + " ms" : (value / 1000).toFixed(1) + " s"; };
const running = () => Boolean(view?.active) || Boolean(view?.launch && !view.launch.exited);

async function post(path, body) {
  busy = true;
  actionError = null;
  render();
  try {
    const res = await fetch(path, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body ?? {}) });
    const value = await res.json();
    if (!res.ok) throw new Error(value.error);
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
    const res = await fetch("/state" + (refresh ? "?refresh" : ""));
    view = await res.json();
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
    '<span class="icon ' + result + '" title="' + LABELS[result] + '">' + ICONS[result] + "</span>" +
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
</script>
</body>
</html>`;

sweepOrphanLogs();

await joinSession({
    canvases: [
        createCanvas({
            id: "auditaur-drill-explorer",
            displayName: "Auditaur Drill Explorer",
            description: "Browse Auditaur drill test suites with each test's latest result, recent history, and flakiness; run or cancel drills.",
            inputSchema: openSchema,
            actions: [
                {
                    name: "get_summary",
                    description: "Suites, tests, the latest run, and which tests are failing, flaky, or have never run.",
                    handler: async (ctx) => summary(await snapshot(entryFor(ctx).state)),
                },
                {
                    name: "refresh",
                    description: "Reload drill files and run results.",
                    handler: async (ctx) => summary(await snapshot(entryFor(ctx).state, true)),
                },
                {
                    name: "run",
                    description: "Start `drill test` in the background for everything, one suite, one test, a grep, or the last failures.",
                    inputSchema: runSchema,
                    handler: async (ctx) => startRun(entryFor(ctx).state, ctx.input ?? {}),
                },
                {
                    name: "cancel",
                    description: "Stop the drill run in progress; it finishes as interrupted.",
                    handler: async (ctx) => cancelRun(entryFor(ctx).state),
                },
            ],
            open: async (ctx) => {
                let entry = servers.get(ctx.instanceId);
                if (!entry) {
                    const state = configure({ launch: null }, ctx.input);
                    entry = { state, ...(await startServer(state)) };
                    servers.set(ctx.instanceId, entry);
                } else if (ctx.input && Object.keys(ctx.input).length > 0) {
                    // Empty input just focuses the panel and keeps its configuration.
                    if (entry.state.starting || (entry.state.launch && !entry.state.launch.exited)) {
                        throw new CanvasError("run_in_progress", "cancel or finish the current run before reconfiguring this canvas");
                    }
                    configure(entry.state, ctx.input);
                }
                return { title: "Drill tests", url: entry.url };
            },
            onClose: async (ctx) => {
                const entry = servers.get(ctx.instanceId);
                if (entry) {
                    servers.delete(ctx.instanceId);
                    await new Promise((resolvePromise) => entry.server.close(() => resolvePromise()));
                }
            },
        }),
    ],
});
