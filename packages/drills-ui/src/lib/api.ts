// The panel server answers `{ error }` with a non-2xx status for anything it refuses.
export async function request<T = unknown>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, init);
  const value = await res.json();
  if (!res.ok || value?.error) throw new Error(value?.error ?? res.statusText);
  return value as T;
}

export const getJson = <T = unknown>(path: string) => request<T>(path);

export const postJson = <T = unknown>(path: string, body?: unknown) =>
  request<T>(path, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body ?? {}) });

export const message = (error: unknown) => (error instanceof Error ? error.message : String(error));
