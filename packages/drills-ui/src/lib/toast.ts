// One short-lived error message for the whole page (e.g. a canvas that failed to open).
type Listener = (text: string | null) => void;

const listeners = new Set<Listener>();
let timer: ReturnType<typeof setTimeout> | undefined;

export function showToast(text: string, durationMs = 6000) {
  for (const listener of listeners) listener(text);
  clearTimeout(timer);
  timer = setTimeout(() => listeners.forEach((listener) => listener(null)), durationMs);
}

export function subscribeToast(listener: Listener) {
  listeners.add(listener);
  return () => void listeners.delete(listener);
}
