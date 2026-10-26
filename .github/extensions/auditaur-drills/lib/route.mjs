// The panel's route: which page it shows and for which run, test, and attempt, as one string like
// `/trace?run=20250101-120000-000&test=checkout&attempt=0`. The server owns it, so the user's clicks
// and the agent's `navigate` are the same write, and it survives a reload.

import { DrillError } from "./errors.mjs";
import { RUN_ID, TEST_ID } from "./paths.mjs";

export const HOOKS_ID = /^hooks:[A-Za-z0-9_-]{1,128}$/;

// Page -> the query parameters it takes. Parameters are optional: no `run` follows the newest run.
export const PAGES = {
    tests: [],
    run: ["run"],
    trace: ["run", "test", "attempt"],
    snapshots: ["run"],
    coverage: ["run"],
    gate: [],
};

export const DEFAULT_ROUTE = "/tests";

const PARAMS = {
    run: (value) => RUN_ID.test(value) ? value : null,
    test: (value) => TEST_ID.test(value) || HOOKS_ID.test(value) ? value : null,
    attempt: (value) => /^\d{1,6}$/.test(value) ? Number(value) : null,
};

const invalid = (message) => new DrillError("invalid_route", message);

// `{ page, params: { run?, test?, attempt? } }`, or a DrillError naming what is wrong.
export function parseRoute(path) {
    if (typeof path !== "string" || path.length > 600) throw invalid("a route is a string like /trace?run=...");
    const url = new URL(path, "http://route");
    const page = url.pathname.replace(/^\/+|\/+$/g, "");
    if (!Object.hasOwn(PAGES, page)) throw invalid(`no page \`${page}\`; pages: ${Object.keys(PAGES).join(", ")}`);
    const params = {};
    for (const [name, raw] of url.searchParams) {
        if (!PAGES[page].includes(name)) throw invalid(`/${page} takes ${PAGES[page].join(", ") || "no parameters"}, not \`${name}\``);
        const value = PARAMS[name](raw);
        if (value === null) throw invalid(`\`${name}\` is not a valid ${name} id`);
        params[name] = value;
    }
    if (params.attempt != null && params.test == null) throw invalid("`attempt` needs a `test`");
    return { page, params };
}

// The one spelling of a route: parameters in page order, empty ones dropped.
export function formatRoute(page, params = {}) {
    const query = new URLSearchParams();
    for (const name of PAGES[page] ?? []) if (params[name] != null) query.set(name, String(params[name]));
    const search = query.toString();
    return `/${page}${search ? `?${search}` : ""}`;
}

export const canonicalRoute = (path) => {
    const { page, params } = parseRoute(path);
    return formatRoute(page, params);
};
