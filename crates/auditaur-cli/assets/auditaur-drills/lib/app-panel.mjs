// One open panel: the shared state, each page's view, and the route that says which page shows.
// Every change (route, view, or data a page should re-read) bumps `version`, notifies the page's
// event stream, and saves the route and views so a rehydrated panel lands where the user left it.

import { checkDbs, sharedState } from "./config.mjs";
import { DrillError } from "./errors.mjs";
import { DEFAULT_ROUTE, PAGES, canonicalRoute, formatRoute, parseRoute } from "./route.mjs";
import { ViewState } from "./view-state.mjs";

const SAVE_DELAY_MS = 250;

const SELECTION = ["runId", "testId", "attempt"];
const NO_SELECTION = { runId: null, testId: null, attempt: null };

// A page's state: the shared state, except `view` is the page's own and the selection (run, test,
// attempt) is the one in the page's last route, so acting on a page that is not on screen acts on
// what that page shows. Other writes go to the shared state, so a run launched from one page is the
// run every page cancels.
function pageState(shared, view, selection) {
    return new Proxy(shared, {
        get: (target, key) => (key === "view" ? view : SELECTION.includes(key) ? selection()[key] : Reflect.get(target, key)),
        set: (target, key, value) => key !== "view" && !SELECTION.includes(key) && Reflect.set(target, key, value),
    });
}

const selectionOf = (route) => {
    const { params } = parseRoute(route);
    return { runId: params.run ?? null, testId: params.test ?? null, attempt: params.attempt ?? null };
};

const tryRoute = (path) => {
    try {
        return path == null ? null : canonicalRoute(path);
    } catch {
        return null;
    }
};

export class AppPanel {
    #pages = new Map();
    #listeners = new Set();
    #save;
    #saveTimer = null;
    // Saves run one after another, each writing the latest snapshot, so an older one never lands last.
    #saving = Promise.resolve();
    #route = DEFAULT_ROUTE;
    #recent = {};
    rev = 0;
    version = 0;

    // `saved` is what an earlier provider stored for this panel. It wins over `input.route`, because
    // a rehydrate replays the original input and must not undo the user's navigation.
    constructor({ features, input, saved, save }) {
        this.input = input ?? {};
        this.shared = sharedState(this.input);
        this.#save = save;
        for (const feature of features) {
            const view = new ViewState(feature.view ?? {});
            this.#pages.set(feature.page, { feature, view, state: pageState(this.shared, view, () => this.#selection(feature.page)) });
        }
        if (saved?.dbs) {
            try {
                this.shared.dbs = checkDbs(saved.dbs);
                for (const db of this.shared.dbs) this.shared.trusted.add(db);
            } catch {
                // Keep the input's stores.
            }
        }
        for (const [page, path] of Object.entries(saved?.recent ?? {})) {
            const route = tryRoute(path);
            if (route && parseRoute(route).page === page) this.#recent[page] = route;
        }
        this.#apply(tryRoute(saved?.route) ?? tryRoute(this.input.route) ?? DEFAULT_ROUTE);
        // After the route, so restored views are not reset by the selection they belong to.
        for (const [page, entry] of this.#pages) entry.view.restore(saved?.views?.[page]);
        for (const entry of this.#pages.values()) entry.view.subscribe(() => this.changed());
    }

    // `{ path, page, rev, recent }`: `recent` is the last route of each page, for its nav tab.
    get route() {
        return { path: this.#route, page: parseRoute(this.#route).page, rev: this.rev, recent: { ...this.#recent } };
    }

    // The page's last route (its current one when on screen).
    routeOf(page) {
        return this.#recent[page] ?? formatRoute(page, {});
    }

    #selection(page) {
        const route = this.#recent[page];
        return route ? selectionOf(route) : NO_SELECTION;
    }

    page(name) {
        const entry = this.#pages.get(name ?? this.route.page);
        if (!entry) throw new DrillError("page_unavailable", `the ${name ?? this.route.page} page failed to load; see the health action`);
        return entry;
    }

    get pages() {
        return [...this.#pages.values()];
    }

    // Shows `path`; throws a DrillError when it is not a valid route.
    navigate(path) {
        const route = canonicalRoute(path);
        this.#apply(route);
        this.rev += 1;
        this.changed();
        return this.route;
    }

    #apply(route) {
        const { page } = parseRoute(route);
        this.#route = route;
        this.#recent[page] = route;
        Object.assign(this.shared, selectionOf(route));
    }

    // Anything a page should re-render for: route, view, or data (a run started, a baseline accepted).
    changed() {
        this.version += 1;
        for (const listener of this.#listeners) listener(this.version);
        this.#scheduleSave();
    }

    subscribe(listener) {
        this.#listeners.add(listener);
        return () => this.#listeners.delete(listener);
    }

    snapshot() {
        return {
            route: this.#route,
            recent: { ...this.#recent },
            views: Object.fromEntries([...this.#pages].map(([page, entry]) => [page, entry.view.get()])),
            dbs: this.shared.dbs,
        };
    }

    #scheduleSave() {
        if (!this.#save || this.#saveTimer) return;
        this.#saveTimer = setTimeout(() => {
            this.#saveTimer = null;
            this.#saving = this.#saving.then(() => this.#save?.(this.snapshot())).catch(() => {});
        }, SAVE_DELAY_MS);
        this.#saveTimer.unref?.();
    }

    // Stops saving, e.g. when the panel closes and its file is removed. Resolves once a save already
    // writing has finished, so removing the file afterwards can't be undone by it.
    dispose() {
        clearTimeout(this.#saveTimer);
        this.#saveTimer = null;
        this.#save = null;
        this.#listeners.clear();
        return this.#saving;
    }
}

export const PAGE_NAMES = Object.keys(PAGES);
