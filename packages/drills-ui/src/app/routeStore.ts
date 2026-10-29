// The route as React sees it. Navigating shows the new page at once; while any navigation is in
// flight the page keeps showing the newest one asked for. Once all have settled, the newest route the
// server committed (from its answers or its event stream, whoever navigated) is what sticks.
import { useSyncExternalStore } from "react";
import { getJson, message, postJson } from "../lib/api";
import { showToast } from "../lib/toast";
import { pathOf, type PageId, type Route } from "./route";
import { onServerEvent } from "./serverEvents";

let current: Route | null = null;
// The newest route the server committed; routes only move forward by revision.
let committed: Route | null = null;
let pending = 0;
const listeners = new Set<() => void>();

function set(route: Route) {
  current = route;
  for (const listener of listeners) listener();
}

function receive(route: Route) {
  if (!committed || route.rev >= committed.rev) committed = route;
  if (pending === 0 && committed !== current) set(committed);
}

export async function startRouteStore() {
  receive(await getJson<Route>("/api/route"));
  onServerEvent((event) => receive(event.route));
}

export async function navigate(path: string) {
  if (!current || path === current.path) return;
  const page = pathOf(path).slice(1) as PageId;
  set({ ...current, path, page, recent: { ...current.recent, [page]: path } });
  pending += 1;
  let route: Route;
  try {
    route = await postJson<Route>("/api/route", { path });
  } catch (err) {
    showToast(`Could not open ${path}: ${message(err)}`);
    route = await getJson<Route>("/api/route").catch(() => committed!);
  }
  pending -= 1;
  receive(route);
}

// Opens `path` in a new panel; this one stays as it is.
export async function openInNewPanel(path: string) {
  try {
    await postJson("/api/panels", { route: path });
  } catch (err) {
    showToast(`Could not open a new panel: ${message(err)}`);
  }
}

const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => void listeners.delete(listener);
};

// The current route; only rendered after startRouteStore resolved.
export function useRoute(): Route {
  return useSyncExternalStore(subscribe, () => current!);
}