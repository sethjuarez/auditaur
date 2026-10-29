// Gate: one human gate of an `auditaur drill` run, answered here instead of in its terminal.
import { Notice, Notices } from "../../components/Notice";
import type { GateAnswer } from "../../components/gate";
import { useAction } from "../../hooks/useAction";
import { useCanvasState } from "../../hooks/useCanvasState";
import { GateBody } from "./GateBody";
import { GateHeader } from "./GateHeader";
import { pollMsFor, type GateState } from "./types";
import "./gate.css";

export function GatePage() {
  const { state, error, reload } = useCanvasState<GateState>(pollMsFor);
  const action = useAction(reload);
  const answer = ({ choiceId, inputs }: GateAnswer) => action.run("/answer", { ...(choiceId ? { choiceId } : {}), inputs });
  return (
    <>
      <GateHeader state={state} />
      <main id="main" className="page-main">
        <Notices>
          {action.error && <Notice tone="error" title={action.error} />}
          {error && <Notice tone="error" title={error} />}
        </Notices>
        {state && <GateBody state={state} answer={answer} />}
      </main>
    </>
  );
}
