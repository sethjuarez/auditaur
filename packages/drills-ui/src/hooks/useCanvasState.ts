// A page's live `/state`: loads it, reloads when the server says something changed, and polls at
// whatever pace the latest state asks for. `change` patches the server-owned view (`state.ui`)
// at once and lets the server's answer, via the event stream, be what sticks.
import { useCallback, useEffect, useRef, useState } from "react";
import { usePageApi } from "../app/PageApi";
import { onServerEvent } from "../app/serverEvents";
import { message } from "../lib/api";
import { canvasStateFor, type ViewPatch } from "../lib/canvasState";

export interface CanvasState<T> {
  state: T | null;
  // Why the last load or view change failed, or null.
  error: string | null;
  reload: (query?: string) => Promise<void>;
  change: (patch: ViewPatch) => void;
}

export function useCanvasState<T extends { ui?: object }>(pollMs: (state: T | null) => number | null): CanvasState<T> {
  const api = canvasStateFor(usePageApi());
  const [state, setState] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Bumped after every load, so polling reschedules even when the state didn't change.
  const [loads, setLoads] = useState(0);
  const pace = useRef(pollMs);
  pace.current = pollMs;

  // At most one load in flight and one queued: a burst of reasons to reload (the server's event, the
  // action that caused it, a poll) costs two requests, not one each. A queued `?refresh` wins.
  const flight = useRef<{ queued: string | null; done: Promise<void> } | null>(null);

  const load = useCallback(async (query: string) => {
    try {
      const next = await api.fetch<T>(query);
      if (!next) return;
      setState(next);
      setError(null);
    } catch (err) {
      setError(`Canvas server unreachable: ${message(err)}`);
    }
    setLoads((count) => count + 1);
  }, [api]);

  const reload = useCallback((query = "") => {
    const running = flight.current;
    if (running) {
      running.queued = running.queued && !query ? running.queued : query;
      return running.done;
    }
    const entry = { queued: null as string | null, done: Promise.resolve() };
    flight.current = entry;
    entry.done = (async () => {
      for (let next: string | null = query; next !== null; next = entry.queued) {
        entry.queued = null;
        await load(next);
      }
      flight.current = null;
    })();
    return entry.done;
  }, [load]);

  const change = useCallback((patch: ViewPatch) => {
    setState((current) => (current ? { ...current, ui: { ...current.ui, ...patch } } : current));
    api.setView(patch).catch((err) => {
      setError(message(err));
      void reload();
    });
  }, [api, reload]);

  useEffect(() => {
    void reload();
    return onServerEvent(() => void reload());
  }, [reload]);

  useEffect(() => {
    const delay = pace.current(state);
    if (delay == null) return;
    const timer = setTimeout(() => void reload(), delay);
    return () => clearTimeout(timer);
  }, [loads, state, reload]);

  return { state, error, reload, change };
}