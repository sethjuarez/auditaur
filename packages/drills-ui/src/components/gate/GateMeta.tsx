// What a gate asks: where it comes from, what to do, what it continues on by itself, and what it
// put on the clipboard.
import { GateCountdown } from "./GateCountdown";
import type { Gate } from "./types";

export function GateMeta({ gate }: { gate: Gate }) {
  const clip = gate.clipboard;
  return (
    <>
      <div className="gate-meta muted">
        {gate.test}
        {gate.since ? ` · waiting since ${new Date(gate.since).toLocaleTimeString()}` : ""}
        {gate.timeoutMs ? <> · <GateCountdown since={gate.since} timeoutMs={gate.timeoutMs} /></> : null}
      </div>
      <p className="gate-instructions">{gate.instructions}</p>
      {gate.selector && (
        <p className="muted gate-note">
          Continues on its own when <span className="path">{gate.selector}</span>
          {gate.expectText ? <> shows “{gate.expectText}”</> : " appears"}.
        </p>
      )}
      {clip && (
        <p className="muted gate-note">
          {clip.label}: {clip.value && !clip.sensitive && !clip.redacted ? <span className="path">{clip.value}</span> : "(hidden)"}
          {` · clipboard ${clip.status}`}
        </p>
      )}
    </>
  );
}
