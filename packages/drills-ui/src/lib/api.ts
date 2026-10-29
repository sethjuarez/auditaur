// The panel server's API. It answers only requests carrying the capability token from the URL the
// panel opened (`?t=`), and answers `{ error }` with a non-2xx status for anything it refuses.
const TOKEN = new URLSearchParams(location.search).get("t") ?? "";

// For URLs the browser fetches itself (event streams, images), which can't carry a header.
export const withToken = (path: string) => `${path}${path.includes("?") ? "&" : "?"}t=${encodeURIComponent(TOKEN)}`;

export async function request<T = unknown>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, { ...init, headers: { ...init?.headers, "X-Drills-Token": TOKEN } });
  const value = await res.json();
  if (!res.ok || value?.error) throw new Error(value?.error ?? res.statusText);
  return value as T;
}

export const getJson = <T = unknown>(path: string) => request<T>(path);

export const postJson = <T = unknown>(path: string, body?: unknown) =>
  request<T>(path, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body ?? {}) });

export const message = (error: unknown) => (error instanceof Error ? error.message : String(error));