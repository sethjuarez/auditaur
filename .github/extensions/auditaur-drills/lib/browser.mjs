// Headless Edge/Chrome screenshots of a panel's loopback page, so the agent can see what the user
// sees. Uses the browser's own `--screenshot` mode: no driver, no dependencies.

import { execFile } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DrillError } from "./errors.mjs";

const CANDIDATES = {
    win32: [
        "Microsoft\\Edge\\Application\\msedge.exe",
        "Google\\Chrome\\Application\\chrome.exe",
    ].flatMap((rel) =>
        [process.env["ProgramFiles(x86)"], process.env.ProgramFiles, process.env.LOCALAPPDATA]
            .filter(Boolean)
            .map((root) => join(root, rel)),
    ),
    darwin: [
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
    ],
    linux: [
        "/usr/bin/microsoft-edge",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
    ],
};

// `AUDITAUR_BROWSER` wins, then the first installed Edge or Chrome.
export function findBrowser() {
    const configured = process.env.AUDITAUR_BROWSER;
    if (configured) return existsSync(configured) ? configured : null;
    return (CANDIDATES[process.platform] ?? CANDIDATES.linux).find((path) => existsSync(path)) ?? null;
}

// Renders the panel page at `url` at `width`x`height` and returns the PNG bytes. `settleMs` gives the
// page's scripts time to fetch and render state before the capture. `?still` tells the page to skip
// its live update stream, which would otherwise hold headless virtual time forever.
export async function captureScreenshot(url, { width = 1280, height = 800, settleMs = 1500, timeoutMs = 30000 } = {}) {
    const still = new URL(url);
    still.searchParams.set("still", "1");
    const browser = findBrowser();
    if (!browser) {
        throw new DrillError(
            "no_browser",
            "no Edge or Chrome found for screenshots; install one or set AUDITAUR_BROWSER to its executable",
        );
    }
    // A rare cold launch stalls; one fresh retry inside the same total budget beats waiting it out.
    const attemptMs = Math.ceil(timeoutMs / 2);
    try {
        return await attempt(browser, still.href, { width, height, settleMs, timeoutMs: attemptMs });
    } catch (error) {
        if (error?.code !== "screenshot_timeout") throw error;
        try {
            return await attempt(browser, still.href, { width, height, settleMs, timeoutMs: attemptMs });
        } catch (retryError) {
            if (retryError?.code !== "screenshot_timeout") throw retryError;
            throw new DrillError("screenshot_failed", `the browser took longer than ${attemptMs} ms, twice`);
        }
    }
}

async function attempt(browser, href, { width, height, settleMs, timeoutMs }) {
    const dir = mkdtempSync(join(tmpdir(), "auditaur-shot-"));
    const file = join(dir, "panel.png");
    try {
        const { failure, timedOut } = await run(browser, [
            "--headless=new",
            "--disable-gpu",
            "--hide-scrollbars",
            "--no-first-run",
            "--no-default-browser-check",
            // A fresh profile's background traffic (updates, SmartScreen, sync) holds virtual time.
            "--disable-background-networking",
            "--disable-component-update",
            "--disable-sync",
            "--disable-extensions",
            "--disable-domain-reliability",
            "--no-pings",
            "--disable-features=msSmartScreenProtection,Translate,OptimizationHints,MediaRouter",
            `--user-data-dir=${join(dir, "profile")}`,
            `--window-size=${width},${height}`,
            `--virtual-time-budget=${settleMs}`,
            `--screenshot=${file}`,
            href,
        ], timeoutMs);
        // Edge sometimes exits non-zero, or hangs on shutdown, after a good capture, so a complete PNG
        // wins over both the exit code and the timeout.
        const png = existsSync(file) ? readFileSync(file) : null;
        if (png && isCompletePng(png)) return png;
        const code = timedOut ? "screenshot_timeout" : "screenshot_failed";
        throw new DrillError(code, failure ?? "the browser exited without writing a screenshot");
    } finally {
        removeLater(dir);
    }
}

const PNG_SIGNATURE = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
const PNG_END = Buffer.from([0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82]); // IEND chunk type + CRC

function isCompletePng(buf) {
    return buf.length > 16 && buf.subarray(0, 8).equals(PNG_SIGNATURE) && buf.subarray(-8).equals(PNG_END);
}

// Best effort: a browser helper may still hold a profile file for a moment, and a leftover temp
// folder must never replace the screenshot or the real error.
function removeLater(dir, tries = 5) {
    try {
        rmSync(dir, { recursive: true, force: true });
    } catch {
        if (tries > 1) setTimeout(() => removeLater(dir, tries - 1), 1000).unref();
    }
}

// Resolves with why the browser failed, if it did, and whether it was killed for running too long.
function run(file, args, timeoutMs) {
    return new Promise((resolve) => {
        execFile(file, args, { timeout: timeoutMs, windowsHide: true }, (error, _stdout, stderr) => {
            if (!error) resolve({});
            else if (error.killed) resolve({ timedOut: true, failure: `the browser took longer than ${timeoutMs} ms` });
            else {
                const detail = String(stderr ?? "").trim().split(/\r?\n/).slice(-3).join(" ");
                resolve({ failure: `the browser failed (${error.code ?? "error"})${detail ? `: ${detail}` : ""}` });
            }
        });
    });
}
