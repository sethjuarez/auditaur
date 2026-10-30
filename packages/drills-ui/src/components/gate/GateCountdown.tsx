// How long a person has left to answer a gate, ticking down from when it started waiting. Without a
// start time it falls back to the gate's whole timeout.
import { useNow } from "../../hooks/useNow";
import { clock, ms } from "../../lib/format";

const URGENT_MS = 60_000;

export function GateCountdown({ since, timeoutMs }: { since: string | null; timeoutMs: number | null }) {
  const started = Date.parse(since ?? "");
  const live = Boolean(timeoutMs) && !Number.isNaN(started);
  const now = useNow(live);
  if (!timeoutMs) return null;
  if (!live) return <span className="gate-countdown">times out after {ms(timeoutMs)}</span>;
  const left = Math.max(0, started + timeoutMs - now);
  return (
    <span className={`gate-countdown nums${left < URGENT_MS ? " urgent" : ""}`} role="timer" title={`Times out after ${ms(timeoutMs)}`}>
      {left > 0 ? `${clock(left)} left` : "timing out…"}
    </span>
  );
}
