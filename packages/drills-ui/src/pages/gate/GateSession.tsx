// Which drill process published the gate, so the user can tell gates of two runs apart.
import type { GateState } from "./types";

export function GateSession({ state }: { state: GateState }) {
  const session = state.session ?? {};
  const rows: [string, string | number | undefined | null][] = [
    ["Request", state.requestId],
    ["Process", session.pid],
    ["Session", session.sessionId],
    ["Telemetry store", session.databasePath],
    ["Times out", state.expiresAt ? new Date(state.expiresAt).toLocaleTimeString() : null],
  ];
  return (
    <dl className="gate-session">
      {rows.filter(([, value]) => value != null && value !== "").map(([term, value]) => (
        <div key={term}>
          <dt className="muted">{term}</dt>
          <dd className="mono">{value}</dd>
        </div>
      ))}
    </dl>
  );
}
