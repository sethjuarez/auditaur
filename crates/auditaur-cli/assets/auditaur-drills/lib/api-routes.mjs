// The HTTP API of one panel, under `/api`:
// - `GET|POST /api/route` reads or changes the routed page; POST answers with the committed route;
// - `GET /api/events` streams `{ version, route }` on every change, so the page reloads;
// - `GET /api/health`, `GET /api/file` (run attachments), `POST /api/cancel`, `POST /api/panels`;
// - `/api/<page>/...` are each page's own routes, plus `GET|POST /api/<page>/view`.
// Every POST tells open pages to reload once it succeeds.

import { serveAttachment } from "./attachments.mjs";
import { health } from "./build-info.mjs";
import { DrillError } from "./errors.mjs";
import { cancelRun } from "./launcher.mjs";
import { STREAMED, readBody } from "./server.mjs";

const KEEPALIVE_MS = 25_000;

function eventStream(panel) {
    return (_url, req, res) => {
        res.writeHead(200, { "Content-Type": "text/event-stream", "Cache-Control": "no-store", "X-Content-Type-Options": "nosniff" });
        const send = () => res.write(`data: ${JSON.stringify({ version: panel.version, route: panel.route })}\n\n`);
        send();
        const unsubscribe = panel.subscribe(send);
        const keepAlive = setInterval(() => res.write(": ping\n\n"), KEEPALIVE_MS);
        req.on("close", () => {
            unsubscribe();
            clearInterval(keepAlive);
        });
        return STREAMED;
    };
}

function stringField(body, name) {
    const value = body[name];
    if (value != null && typeof value !== "string") throw new DrillError("invalid_body", `${name} must be a string`);
    return value ?? null;
}

function pageRoutes({ feature, state, view }) {
    const own = feature.routes(state);
    const routes = {};
    for (const [key, route] of Object.entries(own)) {
        const [method, path] = key.split(" ");
        routes[`${method} /api/${feature.page}${path}`] = route;
    }
    const read = own["GET /state"];
    if (read) routes[`GET /api/${feature.page}/state`] = async (...args) => ({ ...(await read(...args)), ui: view.get() });
    routes[`GET /api/${feature.page}/view`] = () => view.get();
    routes[`POST /api/${feature.page}/view`] = async (_url, req) => {
        const patch = await readBody(req);
        // Bring view scopes up to date first, so the patch isn't undone by a scope change the next read notices.
        await feature.sync?.(state);
        return view.set(patch);
    };
    return routes;
}

const notifying = (panel, route) => async (...args) => {
    const value = await route(...args);
    panel.changed();
    return value;
};

// `openPanel(panel, path)` opens a route in a new panel.
export function apiRoutes(panel, openPanel) {
    const routes = {
        "GET /api/route": () => panel.route,
        "POST /api/route": async (_url, req) => panel.navigate(stringField(await readBody(req), "path")),
        "GET /api/events": eventStream(panel),
        "GET /api/health": async () => ({ ...(await health()), route: panel.route }),
        "GET /api/file": (url) => serveAttachment(panel.shared, url),
        "POST /api/cancel": async (_url, req) => cancelRun(panel.shared, stringField(await readBody(req), "runId")),
        "POST /api/panels": async (_url, req) => openPanel(panel, stringField(await readBody(req), "route")),
    };
    for (const entry of panel.pages) Object.assign(routes, pageRoutes(entry));
    // navigate already notifies; everything else that writes notifies here.
    for (const [key, route] of Object.entries(routes)) {
        if (key.startsWith("POST ") && key !== "POST /api/route") routes[key] = notifying(panel, route);
    }
    return routes;
}
