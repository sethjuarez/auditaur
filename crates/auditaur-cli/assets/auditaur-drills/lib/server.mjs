// One loopback HTTP server per open panel: the app's static assets from `ui/`, plus its API routes.
// The API answers only requests carrying the panel's capability token (the `t` parameter of the URL
// the panel opened, sent back as a header or, for event streams and images, as `t`), addressed to
// this server by its loopback host. Another local page or process that finds the port gets nothing.

import { randomBytes, timingSafeEqual } from "node:crypto";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { DrillError } from "./errors.mjs";

const UI_DIR = join(dirname(fileURLToPath(import.meta.url)), "..", "ui");
const CONTENT_TYPES = {
    ".html": "text/html; charset=utf-8",
    ".css": "text/css; charset=utf-8",
    ".js": "text/javascript; charset=utf-8",
};

export function jsonResponse(res, value, status = 200) {
    res.writeHead(status, { "Content-Type": "application/json; charset=utf-8", "Cache-Control": "no-store" });
    res.end(JSON.stringify(value));
}

const MAX_BODY_BYTES = 1 << 20;
export const TOKEN_HEADER = "x-drills-token";

// A JSON object body; anything else is refused as a DrillError (409) rather than crashing a handler.
export async function readBody(req) {
    const chunks = [];
    let size = 0;
    for await (const chunk of req) {
        size += chunk.length;
        if (size > MAX_BODY_BYTES) throw new DrillError("invalid_body", "request body is too large");
        chunks.push(chunk);
    }
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

// Only the page shell and the React bundle (packages/drills-ui) are served; nothing else under `ui/`
// is reachable. Every non-API path gets the shell, so the client router owns page paths.
const ASSETS = new Map([["/app.js", "app.js"], ["/app.css", "app.css"]]);

const SHELL =
    `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Drills</title>` +
    `<link rel="stylesheet" href="/app.css"></head><body><div id="root"></div>` +
    `<script type="module" src="/app.js"></script></body></html>`;

async function sendAsset(res, asset) {
    const body = asset === null ? SHELL : await readFile(join(UI_DIR, asset));
    const file = asset ?? ".html";
    res.writeHead(200, {
        "Content-Type": CONTENT_TYPES[file.slice(file.lastIndexOf("."))],
        "Cache-Control": "no-store",
        "X-Content-Type-Options": "nosniff",
        // Scripts only from the page's own files; inline styles stay allowed for computed layout, and data:
        // images for Primer's inline icons (the checkbox checkmark).
        "Content-Security-Policy":
            "default-src 'none'; script-src 'self'; connect-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; base-uri 'none'; form-action 'none'",
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

function allowed(req, url, host, token) {
    if (req.headers.host !== host) return false;
    const given = Buffer.from(String(req.headers[TOKEN_HEADER] ?? url.searchParams.get("t") ?? ""));
    return given.length === token.length && timingSafeEqual(given, token);
}

export async function startServer(routes) {
    const token = randomBytes(24).toString("base64url");
    const secret = Buffer.from(token);
    let host = null;
    const server = createServer(async (req, res) => {
        try {
            const url = new URL(req.url ?? "/", "http://127.0.0.1");
            if (req.headers.host !== host) {
                jsonResponse(res, { error: "wrong host" }, 421);
                return;
            }
            if (url.pathname.startsWith("/api/") && !allowed(req, url, host, secret)) {
                jsonResponse(res, { error: "forbidden" }, 403);
                return;
            }
            const route = routes[`${req.method} ${url.pathname}`];
            if (route) {
                const value = await route(url, req, res);
                if (value === STREAMED) return;
                if (value?.[RAW]) sendRaw(res, value[RAW]);
                else jsonResponse(res, value);
            } else if (req.method === "GET" && ASSETS.has(url.pathname)) {
                await sendAsset(res, ASSETS.get(url.pathname));
            } else if (req.method === "GET" && !url.pathname.startsWith("/api/")) {
                await sendAsset(res, null);
            } else {
                jsonResponse(res, { error: "not found" }, 404);
            }
        } catch (error) {
            jsonResponse(res, { error: String(error?.message ?? error) }, error instanceof DrillError ? 409 : 500);
        }
    });
    await new Promise((resolvePromise) => server.listen(0, "127.0.0.1", resolvePromise));
    host = `127.0.0.1:${server.address().port}`;
    return { server, url: `http://${host}/?t=${token}` };
}

// Event streams never end on their own, so open connections are dropped rather than awaited.
export function stopServer(server) {
    return new Promise((resolvePromise) => {
        server.close(() => resolvePromise());
        server.closeAllConnections();
    });
}
