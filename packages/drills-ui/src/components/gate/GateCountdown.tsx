// How long a person has left to answer a gate, ticking down to the runner's deadline (else from
// when it started waiting). Without either it falls back to the gate's whole timeout.
import { useNow } from "../../hooks/useNow";
import { clock, ms } from "../../lib/format";

const URGENT_MS = 60_000;

interface Props { since: string | null; expiresAt: string | null; timeoutMs: number | null }

export function GateCountdown({ since, expiresAt, timeoutMs }: Props) {
  const expires = Date.parse(expiresAt ?? "");
  const deadline = Number.isNaN(expires) ? Date.parse(since ?? "") + (timeoutMs ?? 0) : expires;
  const live = Boolean(timeoutMs) && !Number.isNaN(deadline);
  const now = useNow(live);
  if (!timeoutMs) return null;
  if (!live) return <span className="gate-countdown">times out after {ms(timeoutMs)}</span>;
  const left = Math.max(0, deadline - now);
  return (
    <span className={`gate-countdown nums${left < URGENT_MS ? " urgent" : ""}`} role="timer" title={`Times out after ${ms(timeoutMs)}`}>
      {left > 0 ? `${clock(left)} left` : "timing out…"}
    </span>
  );
}
