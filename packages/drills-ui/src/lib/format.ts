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

// Milliseconds as a clock, rounded up so it never shows 0:00 early: "9:05" or "1:02:03".
export function clock(raw: number): string {
  const total = Math.max(0, Math.ceil(raw / 1000));
  const [h, m, s] = [Math.floor(total / 3600), Math.floor((total % 3600) / 60), total % 60];
  const pad = (n: number) => String(n).padStart(2, "0");
  return h ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}
