// One loopback HTTP server per open panel: the page's static assets from `ui/`, plus its routes.

import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { DrillError } from "./errors.mjs";

const UI_DIR = join(dirname(fileURLToPath(import.meta.url)), "..", "ui");
const SHARED_ASSETS = ["base.css", "common.js"];
const CONTENT_TYPES = {
    ".html": "text/html; charset=utf-8",
    ".css": "text/css; charset=utf-8",
    ".js": "text/javascript; charset=utf-8",
};

export function jsonResponse(res, value, status = 200) {
    res.writeHead(status, { "Content-Type": "application/json; charset=utf-8", "Cache-Control": "no-store" });
    res.end(JSON.stringify(value));
}

export async function readBody(req) {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    const text = Buffer.concat(chunks).toString("utf8");
    return text ? JSON.parse(text) : {};
}

// A route returning `{ [RAW]: { contentType, body } }` sends bytes instead of JSON.
export const RAW = Symbol("raw");

// Only the page's own files and the shared ones are served; nothing else under `ui/` is reachable.
function assetsFor(page) {
    const files = [`${page}.html`, `${page}.css`, `${page}.js`, ...SHARED_ASSETS];
    return new Map(files.map((file) => [file === `${page}.html` ? "/" : `/${file}`, file]));
}

async function sendAsset(res, file) {
    const body = await readFile(join(UI_DIR, file));
    res.writeHead(200, {
        "Content-Type": CONTENT_TYPES[file.slice(file.lastIndexOf("."))],
        "Cache-Control": "no-store",
        "X-Content-Type-Options": "nosniff",
        // Scripts only from the page's own files; inline styles stay allowed for computed layout.
        "Content-Security-Policy":
            "default-src 'none'; script-src 'self'; connect-src 'self'; img-src 'self'; style-src 'self' 'unsafe-inline'; base-uri 'none'; form-action 'none'",
    });
    res.end(body);
}

function sendRaw(res, raw) {
    res.writeHead(200, {
        "Content-Type": raw.contentType,
        "Cache-Control": "no-store",
        "X-Content-Type-Options": "nosniff",
        "Content-Security-Policy": "default-src 'none'; img-src 'self'; style-src 'unsafe-inline'",
    });
    res.end(raw.body);
}

export async function startServer(routes, page) {
    const assets = assetsFor(page);
    const server = createServer(async (req, res) => {
        try {
            const url = new URL(req.url ?? "/", "http://127.0.0.1");
            const route = routes[`${req.method} ${url.pathname}`];
            if (route) {
                const value = await route(url, req);
                if (value?.[RAW]) sendRaw(res, value[RAW]);
                else jsonResponse(res, value);
            } else if (req.method === "GET" && assets.has(url.pathname)) {
                await sendAsset(res, assets.get(url.pathname));
            } else {
                jsonResponse(res, { error: "not found" }, 404);
            }
        } catch (error) {
            jsonResponse(res, { error: String(error?.message ?? error) }, error instanceof DrillError ? 409 : 500);
        }
    });
    await new Promise((resolvePromise) => server.listen(0, "127.0.0.1", resolvePromise));
    return { server, url: `http://127.0.0.1:${server.address().port}/` };
}

export function stopServer(server) {
    return new Promise((resolvePromise) => server.close(() => resolvePromise()));
}
