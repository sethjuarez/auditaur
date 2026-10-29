import { spawn } from "node:child_process";
import { closeSync, openSync, readFileSync } from "node:fs";
import { unlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { RUNS_DIR, CANCEL_REQUEST_FILE } from "./paths.mjs";
import { LOG_PREFIX } from "./processes.mjs";
import { loadSuites } from "./cli.mjs";
import { readAllRuns, readRuns } from "./runs.mjs";
import { DrillError } from "./errors.mjs";

// Workspaces with a launch between its checks and the spawned run's folder appearing.
export const launching = new Set();

export function escapeRegex(value) {
    return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

export function runArgs(state, request) {
    const args = ["drill", "test"];
    if (request.test) {
        const tests = (state.list?.suites ?? []).flatMap((suite) => suite.tests ?? []);
        const match = tests.find((test) => test.id === request.test || test.title === request.test);
        if (!match) {
            throw new DrillError("unknown_test", `no drill test is titled or identified \`${request.test}\``);
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

export async function startRun(state, request) {
    // Claimed synchronously so concurrent requests, from this panel or another one on the same
    // workspace, cannot both pass the checks below.
    if (launching.has(state.workspace) || (state.launch && !state.launch.exited)) {
        throw new DrillError("run_in_progress", "a run started from this canvas is still going");
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

export async function launchRun(state, request) {
    const runs = await readRuns(state);
    if (runs.some((run) => run.status === "running")) {
        throw new DrillError("run_in_progress", "another drill run is in progress; cancel it first");
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

export async function cancelRun(state, runId = null) {
    const running = (await readAllRuns(state)).filter((run) => run.status === "running");
    // Prefer the named run, then the run this canvas launched, then the newest run in the workspace.
    const ours = state.launch && !state.launch.exited ? state.launch.pid : null;
    const active = runId
        ? running.find((run) => run.runId === runId)
        : running.find((run) => run.pid === ours) ?? running[0];
    if (!active) {
        throw new DrillError("no_active_run", "no drill run is in progress");
    }
    await writeFile(join(state.workspace, RUNS_DIR, active.runId, CANCEL_REQUEST_FILE), "");
    return { runId: active.runId, cancelRequested: true };
}
