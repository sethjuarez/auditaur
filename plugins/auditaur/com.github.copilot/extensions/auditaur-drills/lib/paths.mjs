// Where drill files and run folders live, and the ids that may name them.

import { execFileSync } from "node:child_process";
import { existsSync, realpathSync } from "node:fs";
import { basename, dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { DrillError } from "./errors.mjs";

// `<repo>/.github/extensions/auditaur-drills` -> `<repo>`; null for a plugin or user install.
export function projectRepo(extensionDir) {
    const extensions = dirname(extensionDir);
    const github = dirname(extensions);
    const fold = (name) => (process.platform === "win32" ? name.toLowerCase() : name);
    return fold(basename(extensions)) === "extensions" && fold(basename(github)) === ".github" ? dirname(github) : null;
}

// The repository a session runs in: the git root of its working directory, else the directory.
export function sessionRepo(cwd = process.cwd()) {
    try {
        const top = execFileSync("git", ["rev-parse", "--show-toplevel"], {
            cwd,
            encoding: "utf8",
            stdio: ["ignore", "pipe", "ignore"],
            windowsHide: true,
        }).trim();
        return top ? resolve(top) : cwd;
    } catch {
        return cwd;
    }
}

// The copy a repository commits (what `auditaur init extension` writes).
export function projectDir(repo) {
    return join(repo, ".github", "extensions", "auditaur-drills");
}

export function projectCopy(repo) {
    return join(projectDir(repo), "extension.mjs");
}

// One spelling per directory: symlinks and junctions resolved, case folded on Windows.
function canonical(path) {
    let full;
    try {
        full = realpathSync.native(path);
    } catch {
        full = resolve(path);
    }
    return process.platform === "win32" ? full.toLowerCase() : full;
}

// A committed copy serves its own repository. A plugin or user install serves whichever repository
// the session runs in (extensions start in the session's working directory). Two copies would declare
// the same canvas id, so an installed copy steps aside for a committed one.
export function locate(extensionDir, sessionRoot = sessionRepo) {
    const committedIn = projectRepo(extensionDir);
    if (committedIn) {
        return { install: "project", repo: committedIn, defers: false };
    }
    const repo = sessionRoot();
    if (!existsSync(projectCopy(repo))) {
        return { install: "installed", repo, defers: false };
    }
    // A committed copy reached through a symlink or another casing is still this copy.
    if (canonical(projectDir(repo)) === canonical(extensionDir)) {
        return { install: "project", repo, defers: false };
    }
    return { install: "installed", repo, defers: true };
}

const LOCATED = locate(resolve(dirname(fileURLToPath(import.meta.url)), ".."));
export const INSTALL = LOCATED.install;
export const REPO_ROOT = LOCATED.repo;
export const DEFERS_TO_PROJECT = LOCATED.defers;
export const RUNS_DIR = join(".auditaur", "drill-runs");
export const DRILLS_DIR = join(".auditaur", "drills");
export const CANCEL_REQUEST_FILE = "cancel.request";

export const RUN_ID = /^\d{8}-\d{6}-\d{3}[A-Za-z0-9-]*$/;
export const TEST_ID = /^[A-Za-z0-9_-]{1,128}$/;

// Test ids and attempt numbers come from run reports; never let them leave the run folder.
export function attemptDir(testId, attempt) {
    if (!TEST_ID.test(String(testId)) || !Number.isSafeInteger(attempt) || attempt < 0) {
        throw new DrillError("invalid_attempt", "the run report names an invalid test attempt");
    }
    return `tests/${testId}/attempt-${attempt}`;
}

export function workspaceFor(input) {
    const cwd = input?.cwd ?? ".";
    return isAbsolute(cwd) ? cwd : resolve(REPO_ROOT, cwd);
}

// One spelling per workspace folder, for comparing two (Windows paths are case-insensitive).
export function workspaceKey(workspace) {
    const full = resolve(workspace ?? REPO_ROOT).replace(/[\\/]+$/, "");
    return process.platform === "win32" ? full.toLowerCase() : full;
}

export function cliFor(input) {
    const cli = input?.cli ?? process.env.AUDITAUR_BIN ?? "auditaur";
    // A bare command name is looked up on PATH; anything with a separator is a path.
    if (/[\\/]/.test(cli) && !isAbsolute(cli)) {
        return resolve(REPO_ROOT, cli);
    }
    return cli;
}
