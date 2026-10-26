// A page's `/state` and server-owned view, one per page API base (`/api/<page>`).
import { getJson, postJson } from "./api";

export type ViewPatch = Record<string, unknown>;

export class CanvasState {
  #stateSeq = 0;
  #viewEpoch = 0;
  // View fields the page changed but the server hasn't confirmed yet: field -> { value, token }.
  #pendingView = new Map<string, { value: unknown; token: object }>();
  // Patches go out one at a time, so an older patch can never land after a newer one.
  #viewQueue: Promise<unknown> = Promise.resolve();

  constructor(private readonly base: string) {}

  // The page's `/state` (data plus the view as `ui`), or null when a newer request overtook this
  // one. A view change landing mid-request makes it fetch again, and unconfirmed view changes are
  // laid over `ui`, so a poll can't undo what the user just did.
  async fetch<T>(query = ""): Promise<T | null> {
    const mine = ++this.#stateSeq;
    for (;;) {
      const epoch = this.#viewEpoch;
      const value = await getJson<T & { ui?: ViewPatch }>(`${this.base}/state${query}`);
      if (mine !== this.#stateSeq) return null;
      if (epoch !== this.#viewEpoch) continue;
      if (value.ui) for (const [name, pending] of this.#pendingView) value.ui[name] = pending.value;
      return value;
    }
  }

  // Patches the server-owned view; the server then tells every open copy of the page to reload.
  async setView(patch: ViewPatch) {
    const token = {};
    for (const [name, value] of Object.entries(patch)) this.#pendingView.set(name, { value, token });
    const request = this.#viewQueue.then(() => postJson(`${this.base}/view`, patch));
    this.#viewQueue = request.catch(() => {});
    try {
      return await request;
    } finally {
      for (const name of Object.keys(patch)) if (this.#pendingView.get(name)?.token === token) this.#pendingView.delete(name);
      this.#viewEpoch += 1;
    }
  }
}

const byBase = new Map<string, CanvasState>();

export function canvasStateFor(base: string): CanvasState {
  let state = byBase.get(base);
  if (!state) byBase.set(base, (state = new CanvasState(base)));
  return state;
}