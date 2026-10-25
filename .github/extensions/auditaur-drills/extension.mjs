// Extension: auditaur-drills
// Canvases over Auditaur drill tests. They read `auditaur drill ... --json` output and the
// versioned run folders under `.auditaur/drill-runs/`; they never compute results themselves.

import { createServer } from "node:http";
import { execFile, spawn } from "node:child_process";
import { closeSync, openSync, readFileSync } from "node:fs";
import { readdir, readFile, rename, stat, unlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname, isAbsolute, join, resolve } from "node:path";
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

const RUN_ID = /^\d{8}-\d{6}-\d{3}[A-Za-z0-9-]*$/;

async function readRun(state, id) {
    try {
        const report = JSON.parse(await readFile(join(state.workspace, RUNS_DIR, id, "run.json"), "utf8"));
        // Same rule as `drill show`: a running run whose owner died was interrupted.
        if (report.status === "running" && !processAlive(report.pid)) {
            report.status = "interrupted";
            for (const suite of report.suites ?? []) {
                for (const test of suite.tests ?? []) {
                    if (test.status === "running") test.status = "interrupted";
                }
            }
        }
        return report;
    } catch {
        // Missing, being written, or pruned; the next poll picks it up.
        return null;
    }
}

// Every run, newest first. Pruning keeps this short; `HISTORY_RUNS` only limits what is shown.
async function readAllRuns(state) {
    let ids;
    try {
        ids = (await readdir(join(state.workspace, RUNS_DIR), { withFileTypes: true }))
            .filter((entry) => entry.isDirectory() && RUN_ID.test(entry.name))
            .map((entry) => entry.name)
            .sort()
            .reverse();
    } catch {
        return [];
    }
    const runs = [];
    for (const id of ids) {
        const report = await readRun(state, id);
        if (report) runs.push(report);
    }
    return runs;
}

async function readRuns(state) {
    return (await readAllRuns(state)).slice(0, HISTORY_RUNS);
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

async function cancelRun(state, runId = null) {
    const running = (await readAllRuns(state)).filter((run) => run.status === "running");
    // Prefer the named run, then the run this canvas launched, then the newest run in the workspace.
    const ours = state.launch && !state.launch.exited ? state.launch.pid : null;
    const active = runId
        ? running.find((run) => run.runId === runId)
        : running.find((run) => run.pid === ours) ?? running[0];
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

async function startServer(routes, html) {
    const server = createServer(async (req, res) => {
        try {
            const url = new URL(req.url ?? "/", "http://127.0.0.1");
            const route = routes[`${req.method} ${url.pathname}`];
            if (route) {
                jsonResponse(res, await route(url, req));
            } else if (req.method === "GET" && url.pathname === "/") {
                res.writeHead(200, { "Content-Type": "text/html; charset=utf-8", "Cache-Control": "no-store" });
                res.end(html);
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

function explorerRoutes(state) {
    return {
        "GET /state": (url) => snapshot(state, url.searchParams.has("refresh")),
        "POST /run": async (_url, req) => startRun(state, await readBody(req)),
        "POST /cancel": () => cancelRun(state),
    };
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

const liveSchema = {
    type: "object",
    additionalProperties: false,
    properties: {
        cwd: openSchema.properties.cwd,
        runId: { type: "string", pattern: "^\\d{8}-\\d{6}-\\d{3}[A-Za-z0-9-]*$", description: "Show this run instead of following the newest one." },
    },
};

function configureLive(state, input) {
    state.workspace = workspaceFor(input);
    state.runId = input?.runId ?? null;
    return state;
}

async function closeEntry(ctx) {
    const entry = servers.get(ctx.instanceId);
    if (entry) {
        servers.delete(ctx.instanceId);
        await new Promise((resolvePromise) => entry.server.close(() => resolvePromise()));
    }
}

function entryFor(ctx) {
    const entry = servers.get(ctx.instanceId);
    if (!entry) {
        throw new CanvasError("not_open", "this canvas instance is not open");
    }
    return entry;
}

// ---- Live run canvas -------------------------------------------------------------------------

const EVENT_CACHE_LIMIT = 16;
const eventCache = new Map();

const scopeKey = (event) => `${event.suiteId}|${event.testId ?? ""}|${event.attempt ?? ""}`;
const errorText = (error) => (error == null ? null : typeof error === "string" ? error : error.message ?? JSON.stringify(error));

// Steps and pending gates come from the event log; `run.json` only carries test-level state.
function foldEvents(text) {
    const steps = new Map();
    const gates = new Map();
    for (const line of text.split("\n")) {
        let event;
        try {
            event = JSON.parse(line);
        } catch {
            continue; // Blank, or the tail of a line still being written.
        }
        const key = scopeKey(event);
        if (event.type === "stepStarted") {
            if (!steps.has(key)) steps.set(key, []);
            steps.get(key).push({
                index: event.index,
                phase: event.phase,
                title: event.title,
                action: event.action,
                status: "running",
                soft: false,
                startedAt: event.ts,
                durationMs: null,
                error: null,
            });
        } else if (event.type === "stepFinished") {
            const step = (steps.get(key) ?? []).findLast((candidate) => candidate.index === event.index && candidate.status === "running");
            if (step) {
                Object.assign(step, { status: event.status, soft: event.soft, durationMs: event.durationMs, error: errorText(event.error) });
            }
        } else if (event.type === "gateWaiting") {
            gates.set(`${key}|${event.step}`, { ...event, since: event.ts });
        } else if (event.type === "gateResolved") {
            gates.delete(`${key}|${event.step}`);
        }
    }
    return { steps, gates: [...gates.values()] };
}

async function readEvents(path) {
    const info = await stat(path).catch(() => null);
    if (!info) return { steps: new Map(), gates: [] };
    const cached = eventCache.get(path);
    if (cached && cached.size === info.size && cached.mtimeMs === info.mtimeMs) return cached.model;
    const text = await readFile(path, "utf8").catch(() => null);
    if (text == null) return cached?.model ?? { steps: new Map(), gates: [] };
    const model = foldEvents(text);
    eventCache.delete(path);
    eventCache.set(path, { size: info.size, mtimeMs: info.mtimeMs, model });
    if (eventCache.size > EVENT_CACHE_LIMIT) eventCache.delete(eventCache.keys().next().value);
    return model;
}

async function exists(path) {
    return Boolean(await stat(path).catch(() => null));
}

async function pickRun(state) {
    const all = await readAllRuns(state);
    const runs = all.slice(0, HISTORY_RUNS);
    let run;
    if (state.runId) {
        run = all.find((candidate) => candidate.runId === state.runId) ?? (await readRun(state, state.runId));
    } else {
        // Follow the workspace: the run in progress (however old), else the newest one.
        run = all.find((candidate) => candidate.status === "running") ?? all[0] ?? null;
    }
    if (run && !runs.some((candidate) => candidate.runId === run.runId)) runs.push(run);
    return { run, runs };
}

// Pending gates whose request file is still published, with what the panel needs to answer them.
async function pendingGates(run, events) {
    if (run.status !== "running") return [];
    const titles = new Map((run.suites ?? []).flatMap((suite) => (suite.tests ?? []).map((test) => [test.id, test.title])));
    const suites = new Map((run.suites ?? []).map((suite) => [suite.id, suite.suite]));
    const gates = [];
    for (const waiting of events.gates) {
        const request = waiting.gate?.request ?? null;
        if (request && !(await exists(request.requestPath))) continue;
        gates.push({
            requestId: request?.requestId ?? null,
            test: waiting.testId ? titles.get(waiting.testId) ?? waiting.testId : `${suites.get(waiting.suiteId) ?? waiting.suiteId} (hook)`,
            attempt: waiting.attempt,
            since: waiting.since,
            name: waiting.gate?.name,
            instructions: waiting.gate?.instructions,
            selector: waiting.gate?.selector ?? null,
            expectText: waiting.gate?.expectText ?? null,
            timeoutMs: waiting.gate?.timeoutMs ?? null,
            manualContinue: waiting.gate?.manualContinue ?? false,
            choices: waiting.gate?.choices ?? [],
            inputs: waiting.gate?.inputs ?? [],
            clipboard: waiting.gate?.clipboard ?? null,
        });
    }
    return gates;
}

async function liveView(state) {
    const { run, runs } = await pickRun(state);
    const view = {
        workspace: state.workspace,
        pinned: state.runId ?? null,
        runs: runs.map((candidate) => ({ runId: candidate.runId, status: candidate.status })),
        run: null,
        gates: [],
    };
    if (!run) return view;
    const events = await readEvents(join(state.workspace, RUNS_DIR, run.runId, "events.jsonl"));
    const live = run.status === "running";
    const stepsFor = (suiteId, testId, attempt) =>
        (events.steps.get(`${suiteId}|${testId ?? ""}|${attempt ?? ""}`) ?? []).map((step) =>
            // A step left running by a dead or finished run never finished.
            !live && step.status === "running" ? { ...step, status: "interrupted" } : step,
        );
    view.run = {
        runId: run.runId,
        status: run.status,
        startedAt: run.startedAt,
        durationMs: run.durationMs,
        totals: run.totals,
        suites: (run.suites ?? []).map((suite) => ({
            id: suite.id,
            suite: suite.suite,
            path: suite.path,
            status: suite.status,
            durationMs: suite.durationMs,
            error: errorText(suite.error),
            hooks: stepsFor(suite.id, null, null),
            tests: (suite.tests ?? []).map((test) => ({
                id: test.id,
                name: test.name,
                title: test.title,
                tags: test.tags ?? [],
                result: resultOf(test),
                durationMs: test.durationMs,
                attempts: (test.attempts ?? []).map((attempt) => ({
                    attempt: attempt.attempt,
                    status: !live && attempt.status === "running" ? "interrupted" : attempt.status,
                    durationMs: attempt.durationMs,
                    error: errorText(attempt.error),
                    steps: stepsFor(suite.id, test.id, attempt.attempt),
                })),
            })),
        })),
    };
    view.gates = await pendingGates(run, events);
    return view;
}

const GATE_REQUEST_ID = /^[A-Za-z0-9._-]{1,128}$/;

// Answers a gate the shown run is waiting on, with the same response file the gate canvas and
// `respond_human_gate` write. Only gates announced in the run's own event log can be answered.
async function respondGate(state, body) {
    const { run } = await pickRun(state);
    if (!run || run.status !== "running") {
        throw new CanvasError("no_active_run", "the shown run is not waiting on a gate");
    }
    const requestId = String(body?.requestId ?? "");
    if (!GATE_REQUEST_ID.test(requestId)) {
        throw new CanvasError("invalid_gate", "requestId is required");
    }
    const events = await readEvents(join(state.workspace, RUNS_DIR, run.runId, "events.jsonl"));
    const waiting = events.gates.find((candidate) => candidate.gate?.request?.requestId === requestId);
    const request = waiting?.gate?.request;
    if (!request || !(await exists(request.requestPath))) {
        throw new CanvasError("gate_not_pending", `gate \`${requestId}\` is not waiting for an answer`);
    }
    const requestPath = resolve(request.requestPath);
    const responsePath = resolve(request.responsePath);
    const queue = dirname(dirname(requestPath));
    if (
        basename(requestPath) !== `${requestId}.json` ||
        basename(dirname(requestPath)) !== "requests" ||
        basename(queue) !== "human-gates" ||
        responsePath !== join(queue, "responses", `${requestId}.json`)
    ) {
        throw new CanvasError("invalid_gate", "the gate's request is outside a human-gates queue");
    }
    // The published request, not just the event log, must name this gate.
    const published = await readFile(requestPath, "utf8")
        .then((text) => JSON.parse(text))
        .catch(() => null);
    if (published?.requestId !== requestId || published?.nonce !== request.nonce) {
        throw new CanvasError("gate_not_pending", `gate \`${requestId}\` is not waiting for an answer`);
    }
    const gate = waiting.gate;
    const choices = gate.choices ?? [];
    let choiceId = body?.choiceId ?? null;
    if (choices.length) {
        if (!choices.some((choice) => choice.id === choiceId)) {
            throw new CanvasError("invalid_choice", `choose one of: ${choices.map((choice) => choice.id).join(", ")}`);
        }
    } else if (choiceId != null) {
        throw new CanvasError("invalid_choice", "this gate has no choices; omit choiceId to continue");
    }
    const given = Array.isArray(body?.inputs) ? body.inputs : [];
    const inputs = (gate.inputs ?? []).map((input) => ({
        id: input.id,
        value: String(given.find((candidate) => candidate?.id === input.id)?.value ?? ""),
    }));
    const chosen = choices.find((choice) => choice.id === choiceId);
    // The runner checks required inputs for every answer except retry.
    if (chosen?.outcome !== "retry") {
        const missing = (gate.inputs ?? []).filter((input, index) => input.required && !inputs[index].value.trim());
        if (missing.length) {
            throw new CanvasError("missing_input", `fill in: ${missing.map((input) => input.label ?? input.id).join(", ")}`);
        }
    }
    const response = {
        schemaVersion: 1,
        requestId,
        runId: request.runId,
        gateId: request.gateId,
        attempt: request.attempt,
        nonce: request.nonce,
        responder: "human-confirmed-via-drill-run-canvas",
        choiceId,
        inputs,
    };
    const temp = `${responsePath}.${process.pid}.${Date.now()}.tmp`;
    await writeFile(temp, JSON.stringify(response, null, 2), "utf8");
    await rename(temp, responsePath);
    // The gate may have timed out or been cancelled while we wrote: its cleanup already ran, so the
    // request is gone but our answer is not. Drop it (it can hold sensitive inputs). If both are
    // gone, the runner consumed the answer.
    if (!(await exists(requestPath)) && (await unlink(responsePath).then(() => true, () => false))) {
        throw new CanvasError("gate_not_pending", `gate \`${requestId}\` stopped waiting before the answer arrived`);
    }
    return { requestId, choiceId, answered: true };
}

function liveRoutes(state) {
    return {
        "GET /state": () => liveView(state),
        "POST /gate": async (_url, req) => respondGate(state, await readBody(req)),
        "POST /cancel": async () => {
            const { run } = await pickRun(state);
            if (!run || run.status !== "running") {
                throw new CanvasError("no_active_run", "the shown run is not in progress");
            }
            return cancelRun(state, run.runId);
        },
        "POST /follow": async (_url, req) => {
            const runId = (await readBody(req)).runId ?? null;
            if (runId != null && !RUN_ID.test(runId)) {
                throw new CanvasError("invalid_run", "not a drill run id");
            }
            state.runId = runId;
            return { pinned: state.runId };
        },
    };
}

function liveSummary(view) {
    const run = view.run;
    if (!run) return { workspace: view.workspace, run: null, gates: [] };
    const tests = run.suites.flatMap((suite) => suite.tests);
    return {
        runId: run.runId,
        status: run.status,
        totals: run.totals,
        durationMs: run.durationMs,
        running: tests.filter((test) => test.result === "running").map((test) => test.title),
        failed: tests
            .filter((test) => test.result === "failed")
            .map((test) => ({ title: test.title, error: test.attempts.at(-1)?.error ?? null })),
        gates: view.gates.map(({ requestId, test, name, instructions, choices, inputs }) => ({
            requestId,
            test,
            name,
            instructions,
            choices,
            inputs: inputs.map(({ id, label, kind, required }) => ({ id, label, kind, required })),
        })),
    };
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

const LIVE_HTML = String.raw`<!doctype html>
<html>
<head>
<meta charset="utf-8">
<title>Drill run</title>
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
  h2 { margin: 0 0 4px; font-size: 14px; font-weight: var(--font-weight-semibold, 600); }
  .muted { color: var(--text-color-muted, #656d76); }
  .row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .spacer { flex: 1; }
  button, select, input {
    font: inherit; font-size: 12px; padding: 2px 10px; border-radius: 6px; color: inherit;
    border: 1px solid var(--border-color-default, #d0d7de); background: transparent;
  }
  button { cursor: pointer; }
  button:hover:not(:disabled) { border-color: var(--text-color-muted, #656d76); }
  button:disabled { opacity: .5; cursor: default; }
  button.primary { background: var(--true-color-blue, #0969da); border-color: var(--true-color-blue, #0969da); color: var(--color-white, #fff); }
  button:focus-visible, select:focus-visible, input:focus-visible, .test:focus-visible { outline: 2px solid var(--color-focus-outline, #0969da); outline-offset: 1px; }
  main { padding: 0 16px 24px; }
  .path { font-family: var(--font-mono, Consolas, monospace); font-size: 11px; }
  .banner { margin-top: 8px; padding: 6px 10px; border-radius: 6px; font-size: 12px; white-space: pre-wrap; }
  .banner.error { background: var(--true-color-red-muted, #ffebe9); }
  .bar { height: 4px; border-radius: 2px; background: var(--border-color-default, #d0d7de); margin-top: 8px; overflow: hidden; display: flex; }
  .bar > div { height: 100%; transition: width .3s; }
  .gate { margin-top: 12px; padding: 10px 12px; border-radius: 8px; border: 2px solid var(--true-color-orange, #bc4c00); background: var(--true-color-orange-muted, #fff1e5); }
  .gate p { margin: 4px 0; white-space: pre-wrap; }
  .gate label { display: block; font-size: 12px; margin-top: 6px; }
  .gate input { display: block; width: 100%; box-sizing: border-box; margin-top: 2px; padding: 4px 8px; }
  .gate .row { margin-top: 8px; }
  .suite { margin-top: 12px; }
  .suite-head { padding: 6px 0; border-bottom: 1px solid var(--border-color-default, #d0d7de); }
  .suite-name { font-weight: var(--font-weight-semibold, 600); }
  .test { display: grid; grid-template-columns: 18px 1fr auto; gap: 8px; align-items: center; padding: 4px 0 4px 4px; border-bottom: 1px solid var(--border-color-default, #d0d7de); cursor: pointer; }
  .test:hover { background: color-mix(in srgb, var(--text-color-muted, #656d76) 8%, transparent); }
  .icon { text-align: center; font-weight: 700; }
  .tag { font-size: 11px; padding: 0 6px; border-radius: 10px; border: 1px solid var(--border-color-default, #d0d7de); margin-left: 4px; white-space: nowrap; }
  .duration { font-size: 11px; min-width: 48px; text-align: right; }
  .detail { font-size: 12px; padding: 2px 0 8px 30px; border-bottom: 1px solid var(--border-color-default, #d0d7de); }
  .attempt-head { margin-top: 4px; font-weight: var(--font-weight-semibold, 600); }
  .step { display: grid; grid-template-columns: 16px auto 1fr auto; gap: 6px; align-items: baseline; padding: 1px 0; }
  .phase { font-size: 11px; }
  .error { color: var(--true-color-red, #cf222e); white-space: pre-wrap; font-family: var(--font-mono, Consolas, monospace); font-size: 11px; }
  .step .error { grid-column: 3 / 5; }
  .passed { color: var(--true-color-green, #1a7f37); }
  .failed, .timedOut { color: var(--true-color-red, #cf222e); }
  .flaky { color: var(--true-color-orange, #bc4c00); }
  .interrupted { color: var(--true-color-yellow, #9a6700); }
  .running { color: var(--true-color-blue, #0969da); }
  .skipped, .notRun, .pending { color: var(--text-color-muted, #656d76); }
  .empty { padding: 24px 0; text-align: center; }
</style>
</head>
<body>
<header>
  <div class="row">
    <h1>Drill run</h1>
    <span id="status"></span>
    <span class="spacer"></span>
    <select id="pick" aria-label="Run to show"></select>
    <button id="cancel" hidden>Cancel run</button>
  </div>
  <div id="summary" class="muted" style="font-size:12px;margin-top:4px"></div>
  <div id="bar" class="bar" hidden></div>
  <div id="banners" aria-live="polite"></div>
</header>
<main>
  <div id="gates" aria-live="assertive"></div>
  <div id="tree"><div class="empty muted">Loading…</div></div>
</main>
<script>
const ICONS = { passed: "✓", failed: "✗", timedOut: "⏱", flaky: "◐", interrupted: "!", running: "●", skipped: "⊘", notRun: "·", pending: "·" };
const toggled = new Map();
let view = null;
let actionError = null;
let gateSig = null;
let timer = null;

const esc = (value) => String(value ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const ms = (raw) => { const value = Number(raw); return raw == null || Number.isNaN(value) ? "" : value < 1000 ? Math.round(value) + " ms" : (value / 1000).toFixed(1) + " s"; };
const since = (ts) => { const start = Date.parse(ts); return Number.isNaN(start) ? null : Math.max(0, Date.now() - start); };
const cls = (status) => (ICONS[status] ? status : "notRun");
const icon = (status) => '<span class="icon ' + cls(status) + '" title="' + esc(status) + '">' + ICONS[cls(status)] + "</span>";

async function post(path, body) {
  actionError = null;
  try {
    const res = await fetch(path, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body ?? {}) });
    const value = await res.json();
    if (!res.ok) throw new Error(value.error);
    return value;
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
    view = await (await fetch("/state")).json();
    if (view.error) throw new Error(view.error);
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
</script>
</body>
</html>`;

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
                    entry = { state, ...(await startServer(explorerRoutes(state), EXPLORER_HTML)) };
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
            onClose: closeEntry,
        }),
        createCanvas({
            id: "auditaur-drill-run",
            displayName: "Auditaur Drill Run",
            description: "Watch an Auditaur drill run live, step by step, and answer its human gates; follows the newest run unless given a runId.",
            inputSchema: liveSchema,
            actions: [
                {
                    name: "get_status",
                    description: "The shown run's status, totals, running and failed tests, and gates waiting for an answer.",
                    handler: async (ctx) => liveSummary(await liveView(entryFor(ctx).state)),
                },
                {
                    name: "respond_gate",
                    description: "Answer a gate the shown run is waiting on. Omit choiceId for gates without choices.",
                    inputSchema: {
                        type: "object",
                        additionalProperties: false,
                        required: ["requestId"],
                        properties: {
                            requestId: { type: "string" },
                            choiceId: { type: "string" },
                            inputs: {
                                type: "array",
                                items: {
                                    type: "object",
                                    additionalProperties: false,
                                    required: ["id", "value"],
                                    properties: { id: { type: "string" }, value: { type: "string" } },
                                },
                            },
                        },
                    },
                    handler: async (ctx) => respondGate(entryFor(ctx).state, ctx.input ?? {}),
                },
                {
                    name: "cancel",
                    description: "Stop the shown run; it finishes as interrupted.",
                    handler: async (ctx) => liveRoutes(entryFor(ctx).state)["POST /cancel"](),
                },
            ],
            open: async (ctx) => {
                let entry = servers.get(ctx.instanceId);
                if (!entry) {
                    const state = configureLive({}, ctx.input);
                    entry = { state, ...(await startServer(liveRoutes(state), LIVE_HTML)) };
                    servers.set(ctx.instanceId, entry);
                } else if (ctx.input && Object.keys(ctx.input).length > 0) {
                    configureLive(entry.state, ctx.input);
                }
                return { title: "Drill run", url: entry.url };
            },
            onClose: closeEntry,
        }),
    ],
});
