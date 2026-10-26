// A text file from the panel server, fetched once the first time `enabled` turns true (e.g. when a
// disclosure opens). JSON comes back pretty-printed; very large files are cut short.
import { useEffect, useState } from "react";
import { message } from "../lib/api";

const LIMIT = 200_000;

export interface FileText {
  text: string | null;
  // Parsed JSON, or undefined when the file isn't JSON.
  json: unknown;
  error: string | null;
}

const PENDING: FileText = { text: null, json: undefined, error: null };

export function useFileText(url: string, enabled: boolean): FileText {
  // Results and the request flag are tied to their url, so a new url never shows the old file.
  const [loaded, setLoaded] = useState<{ url: string; result: FileText } | null>(null);
  const [requestedUrl, setRequestedUrl] = useState<string | null>(null);
  if (enabled && requestedUrl !== url) setRequestedUrl(url);
  const requested = requestedUrl === url;

  useEffect(() => {
    if (!requested) return;
    let live = true;
    const abort = new AbortController();
    const setResult = (result: FileText) => live && setLoaded({ url, result });
    (async () => {
      try {
        const res = await fetch(url, { signal: abort.signal });
        const body = await res.text();
        if (!res.ok) throw new Error(errorOf(body) ?? res.statusText);
        let json: unknown;
        let shown = body;
        try {
          json = JSON.parse(body);
          shown = JSON.stringify(json, null, 2);
        } catch {
          json = undefined;
        }
        setResult({ text: shown.length > LIMIT ? `${shown.slice(0, LIMIT)}\n…` : shown, json, error: null });
      } catch (err) {
        if (!abort.signal.aborted) setResult({ text: null, json: undefined, error: message(err) });
      }
    })();
    return () => {
      live = false;
      abort.abort();
    };
  }, [url, requested]);

  return loaded?.url === url ? loaded.result : PENDING;
}

function errorOf(body: string): string | null {
  try {
    return JSON.parse(body)?.error ?? null;
  } catch {
    return null;
  }
}
