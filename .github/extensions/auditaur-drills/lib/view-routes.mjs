// The HTTP side of a panel's view state, added to every canvas's own routes:
// - `GET /state` answers include the view as `ui`, so a page renders data and view together;
// - `GET /view` / `POST /view` read and patch the view;
// - `GET /events` streams the view's version, so pages reload when the agent changes something;
// - every POST (the page doing something) notifies those streams once it succeeds.

import { STREAMED, readBody } from "./server.mjs";

function eventStream(view) {
    return (_url, req, res) => {
        res.writeHead(200, { "Content-Type": "text/event-stream", "Cache-Control": "no-store", "X-Content-Type-Options": "nosniff" });
        const send = (version) => res.write(`data: ${version}\n\n`);
        send(view.version);
        const unsubscribe = view.subscribe(send);
        const keepAlive = setInterval(() => res.write(": ping\n\n"), 25_000);
        req.on("close", () => {
            unsubscribe();
            clearInterval(keepAlive);
        });
        return STREAMED;
    };
}

const notifying = (view, route) => async (...args) => {
    const value = await route(...args);
    view.changed();
    return value;
};

// `sync()` brings view scopes up to date before a page's view patch applies, so the patch isn't
// undone by a scope change the next read would notice.
export function withViewRoutes(routes, view, sync = async () => {}) {
    const wrapped = Object.fromEntries(
        Object.entries(routes).map(([key, route]) => [key, key.startsWith("POST ") ? notifying(view, route) : route]),
    );
    const state = routes["GET /state"];
    if (state) {
        wrapped["GET /state"] = async (...args) => {
            const value = await state(...args);
            return { ...value, ui: view.get() };
        };
    }
    return {
        ...wrapped,
        "GET /view": () => view.get(),
        "POST /view": notifying(view, async (_url, req) => {
            const patch = await readBody(req);
            await sync();
            return view.set(patch);
        }),
        "GET /events": eventStream(view),
    };
}
