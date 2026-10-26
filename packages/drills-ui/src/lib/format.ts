// Milliseconds as "840 ms" or "1.2 s".
export function ms(raw: unknown, digits = 1): string {
  const value = Number(raw);
  if (raw == null || Number.isNaN(value)) return "";
  return value < 1000 ? `${Math.round(value)} ms` : `${(value / 1000).toFixed(digits)} s`;
}

// Milliseconds since an ISO timestamp, or null.
export function since(ts: string | null | undefined): number | null {
  const start = Date.parse(ts ?? "");
  return Number.isNaN(start) ? null : Math.max(0, Date.now() - start);
}

export const plural = (count: number, word: string) => `${count} ${word}${count === 1 ? "" : "s"}`;
