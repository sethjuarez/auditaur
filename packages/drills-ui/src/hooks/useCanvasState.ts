// A page's live `/state`: loads it, reloads when the server says something changed, and polls at
// whatever pace the latest state asks for. `change` patches the server-owned view (`state.ui`)
// at once and lets the server's answer, via the event stream, be what sticks.
import { useCallback, useEffect, useRef, useState } from "react";
import { message } from "../lib/api";
import { fetchState, onServerChange, setView, type ViewPatch } from "../lib/canvasState";

export interface CanvasState<T> {
  state: T | null;
  // Why the last load or view change failed, or null.
  error: string | null;
  reload: (query?: string) => Promise<void>;
  change: (patch: ViewPatch) => void;
}

export function useCanvasState<T extends { ui?: object }>(pollMs: (state: T | null) => number | null): CanvasState<T> {
  const [state, setState] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Bumped after every load, so polling reschedules even when the state didn't change.
  const [loads, setLoads] = useState(0);
  const pace = useRef(pollMs);
  pace.current = pollMs;

  const reload = useCallback(async (query = "") => {
    try {
      const next = await fetchState<T>(query);
      if (!next) return;
      setState(next);
      setError(null);
    } catch (err) {
      setError(`Canvas server unreachable: ${message(err)}`);
    }
    setLoads((count) => count + 1);
  }, []);

  const change = useCallback((patch: ViewPatch) => {
    setState((current) => (current ? { ...current, ui: { ...current.ui, ...patch } } : current));
    setView(patch).catch((err) => {
      setError(message(err));
      void reload();
    });
  }, [reload]);

  useEffect(() => {
    void reload();
    return onServerChange(() => void reload());
  }, [reload]);

  useEffect(() => {
    const delay = pace.current(state);
    if (delay == null) return;
    const timer = setTimeout(() => void reload(), delay);
    return () => clearTimeout(timer);
  }, [loads, state, reload]);

  return { state, error, reload, change };
}
