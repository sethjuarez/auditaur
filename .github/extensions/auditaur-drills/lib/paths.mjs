// Where drill files and run folders live, and the ids that may name them.

import { dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { DrillError } from "./errors.mjs";

// `.github/extensions/auditaur-drills/lib/paths.mjs` -> repository root.
export const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");
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

export function cliFor(input) {
    const cli = input?.cli ?? process.env.AUDITAUR_BIN ?? "auditaur";
    // A bare command name is looked up on PATH; anything with a separator is a path.
    if (/[\\/]/.test(cli) && !isAbsolute(cli)) {
        return resolve(REPO_ROOT, cli);
    }
    return cli;
}
