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

const toItem = (store: AppStore): ChecklistItem => ({
  value: store.db,
  label: store.serviceName ?? "app",
  detail: store.sessionId ? store.sessionId + ((store.sessions ?? 1) > 1 ? ` +${(store.sessions ?? 1) - 1}` : "") : store.db,
  badge: store.status,
});

export function useAppStores(open: boolean, inUse: string[]) {
  const [found, setFound] = useState<AppStore[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    let live = true;
    getJson<{ stores: AppStore[]; error: string | null }>("/apps").then(
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

  if (!found) return { items: null, error };
  const known = new Set(found.map((store) => store.db));
  const extra = inUse.filter((db) => !known.has(db)).map((db) => ({ db, serviceName: "chosen store" }));
  return { items: [...found, ...extra].map(toItem), error };
}
