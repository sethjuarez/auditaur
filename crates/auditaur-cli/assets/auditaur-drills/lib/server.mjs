// One loopback HTTP server per open panel: the page's static assets from `ui/`, plus its routes.

import { createServer } from "node:http";
import { existsSync } from "node:fs";
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

// A JSON object body; anything else is refused as a DrillError (409) rather than crashing a handler.
export async function readBody(req) {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    const text = Buffer.concat(chunks).toString("utf8");
    if (!text) return {};
    let body;
    try {
        body = JSON.parse(text);
    } catch {
        throw new DrillError("invalid_body", "request body is not JSON");
    }
    if (body === null || typeof body !== "object" || Array.isArray(body)) {
        throw new DrillError("invalid_body", "request body must be a JSON object");
    }
    return body;
}

// A route returning `{ [RAW]: { contentType, body } }` sends bytes instead of JSON.
export const RAW = Symbol("raw");
// A route returning `STREAMED` has written (and keeps writing) the response itself.
export const STREAMED = Symbol("streamed");

// Only the page's own files, the UI components it names (`<name>.js` and `.css`), and the shared ones
// are served; nothing else under `ui/` is reachable. A page without its own `<page>.html` is part of
// the React bundle (packages/drills-ui): a generated shell plus `app.js` and `app.css`.
function assetsFor(page, components) {
    if (!existsSync(join(UI_DIR, `${page}.html`))) {
        return new Map([["/", { shell: page }], ["/app.js", "app.js"], ["/app.css", "app.css"]]);
    }
    const files = [`${page}.html`, `${page}.css`, `${page}.js`, ...SHARED_ASSETS, ...components.flatMap((name) => [`${name}.js`, `${name}.css`])];
    return new Map(files.map((file) => [file === `${page}.html` ? "/" : `/${file}`, file]));
}

const shell = (page) =>
    `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Drills</title>` +
    `<link rel="stylesheet" href="app.css"></head><body><div id="root" data-page="${page}"></div>` +
    `<script type="module" src="app.js"></script></body></html>`;

async function sendAsset(res, asset) {
    const body = asset.shell ? shell(asset.shell) : await readFile(join(UI_DIR, asset));
    const file = asset.shell ? ".html" : asset;
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

export async function startServer(routes, page, components = []) {
    const assets = assetsFor(page, components);
    const server = createServer(async (req, res) => {
        try {
            const url = new URL(req.url ?? "/", "http://127.0.0.1");
            const route = routes[`${req.method} ${url.pathname}`];
            if (route) {
                const value = await route(url, req, res);
                if (value === STREAMED) return;
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

// Event streams never end on their own, so open connections are dropped rather than awaited.
export function stopServer(server) {
    return new Promise((resolvePromise) => {
        server.close(() => resolvePromise());
        server.closeAllConnections();
    });
}
