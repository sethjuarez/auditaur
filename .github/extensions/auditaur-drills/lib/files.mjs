import { readFile, stat } from "node:fs/promises";

export async function readJson(path) {
    return JSON.parse(await readFile(path, "utf8"));
}

export async function exists(path) {
    return Boolean(await stat(path).catch(() => null));
}
