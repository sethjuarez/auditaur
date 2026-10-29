import { after } from "node:test";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

// A fresh temp directory, removed once the test file finishes.
const temps = [];
after(() => Promise.all(temps.map((dir) => rm(dir, { recursive: true, force: true }))));
export async function tempDir(prefix) {
    const dir = await mkdtemp(join(tmpdir(), prefix));
    temps.push(dir);
    return dir;
}

// The extension's modules, by path from this package.
export const LIB = new URL("../../../../.github/extensions/auditaur-drills/lib/", import.meta.url);
export const lib = (name) => import(new URL(name, LIB));

export const RUN = "20250101-120000-000";