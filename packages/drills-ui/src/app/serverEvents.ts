// The panel's one event stream (`/api/events`): `{ version, route }` whenever the route, a view, or
// data changed, whoever changed it (the user, another copy of the page, or the agent).
import { withToken } from "../lib/api";
import type { Route } from "./route";

export interface ServerEvent {
  version: number;
  route: Route;
}

type Listener = (event: ServerEvent) => void;
const listeners = new Set<Listener>();
let source: EventSource | null = null;

// A still capture (`?still`, see lib/browser.mjs) skips live updates: an open event stream would
// stop headless virtual time, and a still image needs none.
export const isStill = () => new URLSearchParams(location.search).has("still");

export function onServerEvent(listener: Listener): () => void {
  listeners.add(listener);
  if (!source && !isStill()) {
    source = new EventSource(withToken("/api/events"));
    source.onmessage = (message) => {
      const event = JSON.parse(message.data) as ServerEvent;
      for (const each of listeners) each(event);
    };
  }
  return () => void listeners.delete(listener);
}