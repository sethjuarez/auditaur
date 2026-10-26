// The panel's `/state` and server-owned view. A page has exactly one of these, so it is a module.
import { getJson, postJson } from "./api";

export type ViewPatch = Record<string, unknown>;

let stateSeq = 0;
let viewEpoch = 0;
// View fields the page changed but the server hasn't confirmed yet: field -> { value, token }.
const pendingView = new Map<string, { value: unknown; token: object }>();

// The panel's `/state` (data plus the view as `ui`), or null when a newer request overtook this one.
// A view change landing mid-request makes it fetch again, and unconfirmed view changes are laid over
// `ui`, so a poll can't undo what the user just did.
export async function fetchState<T>(query = ""): Promise<T | null> {
  const mine = ++stateSeq;
  for (;;) {
    const epoch = viewEpoch;
    const value = await getJson<T & { ui?: ViewPatch }>("/state" + query);
    if (mine !== stateSeq) return null;
    if (epoch !== viewEpoch) continue;
    if (value.ui) for (const [name, pending] of pendingView) value.ui[name] = pending.value;
    return value;
  }
}

// Patches go out one at a time, so an older patch can never land after a newer one.
let viewQueue: Promise<unknown> = Promise.resolve();

// Patches the server-owned view; the server then tells every open copy of the page to reload.
export async function setView(patch: ViewPatch) {
  const token = {};
  for (const [name, value] of Object.entries(patch)) pendingView.set(name, { value, token });
  const request = viewQueue.then(() => postJson("/view", patch));
  viewQueue = request.catch(() => {});
  try {
    return await request;
  } finally {
    for (const name of Object.keys(patch)) if (pendingView.get(name)?.token === token) pendingView.delete(name);
    viewEpoch += 1;
  }
}

// A still capture (`?still`, see lib/browser.mjs) skips live updates: an open event stream would
// stop headless virtual time, and a still image needs none.
export const isStill = () => new URLSearchParams(location.search).has("still");

// Calls `reload` whenever the server says the view or data changed, e.g. because the agent acted.
export function onServerChange(reload: () => void): () => void {
  if (isStill()) return () => {};
  const source = new EventSource("/events");
  source.onmessage = () => reload();
  return () => source.close();
}
