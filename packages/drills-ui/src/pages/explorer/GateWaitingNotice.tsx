// The Tests page's "waiting for you" banner: a run is paused on a human gate, how long is left, and
// a button to the Progress page, where the gate is answered.
import { Notice } from "../../components/Notice";
import { OpenInLinks } from "../../components/OpenIn";
import { GateCountdown } from "../../components/gate";
import type { ExplorerGate } from "./types";

export function GateWaitingNotice({ runId, gates }: { runId: string; gates: ExplorerGate[] }) {
  if (!gates.length) return null;
  const [first] = gates;
  const more = gates.length > 1 ? ` (+${gates.length - 1} more)` : "";
  return (
    <Notice
      tone="warning"
      className="gate-waiting"
      title={<>Waiting for you: {first.name || "human gate"} · {first.test}{more}</>}
    >
      <span className="gate-waiting-body">
        <GateCountdown since={first.since} timeoutMs={first.timeoutMs} />
        <OpenInLinks caption="Answer it in" links={[{ to: "run", runId }]} />
      </span>
    </Notice>
  );
}
