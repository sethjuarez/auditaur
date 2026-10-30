// The app telemetry stores the picker offers: what `auditaur apps` discovered, plus any store already
// in use that it didn't (one the agent chose). Loaded each time the picker opens.
import { useEffect, useState } from "react";
import type { ChecklistItem } from "../../components/Checklist";
import { getJson, message } from "../../lib/api";

interface AppStore {
  db: string;
  sessionId?: string;
  serviceName?: string;
  status?: string;
  sessions?: number;
}

const toItem = (store: AppStore, ours: boolean): ChecklistItem => ({
  value: store.db,
  label: store.serviceName ?? "app",
  detail: store.sessionId ? store.sessionId + ((store.sessions ?? 1) > 1 ? ` +${(store.sessions ?? 1) - 1}` : "") : store.db,
  badge: ours ? "this run's app" : store.status,
});

// The run's own app first (stores arrive newest first, which is kept within each group).
export function useAppStores(open: boolean, inUse: string[], apps: string[]) {
  const [found, setFound] = useState<AppStore[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    let live = true;
    getJson<{ stores: AppStore[]; error: string | null }>("/api/coverage/apps").then(
      (result) => {
        if (!live) return;
        setFound(result.stores);
        setError(result.error);
      },
      (err) => {
        if (!live) return;
        setFound([]);
        setError(message(err));
      },
    );
    return () => {
      live = false;
    };
  }, [open]);

  if (!found) return { items: null, suggested: [] as string[], error };
  const known = new Set(found.map((store) => store.db));
  const extra = inUse.filter((db) => !known.has(db)).map((db) => ({ db, serviceName: "chosen store" }));
  const ours = (store: AppStore) => Boolean(store.serviceName && apps.includes(store.serviceName));
  const all = [...found, ...extra];
  const ordered = [...all.filter(ours), ...all.filter((store) => !ours(store))];
  return {
    items: ordered.map((store) => toItem(store, ours(store))),
    suggested: all.filter(ours).map((store) => store.db),
    error,
  };
}
