// A page's POST actions (run, cancel, accept…): one at a time, with the last failure kept for a
// notice, then a reload so the page shows what the action did. Resolves to whether it succeeded.
import { useCallback, useState } from "react";
import { message, postJson } from "../lib/api";

export function useAction(reload: () => Promise<void>) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = useCallback(async (path: string, body?: unknown) => {
    setBusy(true);
    setError(null);
    let ok = true;
    try {
      await postJson(path, body);
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
  }, [reload]);
  return { busy, error, run };
}