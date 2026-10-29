// A page's POST actions (page-relative like `/run`, or app-wide like `/api/cancel`): one at a
// time, with the last failure kept for a notice, then a reload so the page shows what the action
// did. Resolves to whether it succeeded; `done` sees the server's answer before the reload.
import { useCallback, useState } from "react";
import { apiPath, usePageApi } from "../app/PageApi";
import { message, postJson } from "../lib/api";

export function useAction(reload: () => Promise<void>) {
  const base = usePageApi();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = useCallback(async <T = unknown>(path: string, body?: unknown, done?: (result: T) => void) => {
    setBusy(true);
    setError(null);
    let ok = true;
    try {
      const result = await postJson<T>(apiPath(base, path), body);
      done?.(result);
    } catch (err) {
      ok = false;
      setError(message(err));
    } finally {
      // Stay busy until the reload lands, so buttons never re-enable against stale state.
      try {
        await reload();
      } finally {
        setBusy(false);
      }
    }
    return ok;
  }, [base, reload]);
  const clearError = useCallback(() => setError(null), []);
  return { busy, error, run, clearError };
}