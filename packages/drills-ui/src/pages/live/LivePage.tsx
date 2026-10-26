// Progress: one drill run live, step by step, with its human gates to answer; follows the newest
// run unless one is picked.
import { Notice, Notices } from "../../components/Notice";
import { useAction } from "../../hooks/useAction";
import { useCanvasState } from "../../hooks/useCanvasState";
import type { GateAnswer } from "./GateForm";
import { GateList } from "./GateList";
import { LiveHeader } from "./LiveHeader";
import { RunTree } from "./RunTree";
import { isLive, type LiveState } from "./types";
import "./live.css";

export function LivePage() {
  const { state, error, reload, change } = useCanvasState<LiveState>((current) => (isLive(current) ? 500 : 3000));
  const action = useAction(reload);
  const toggle = (id: string, open: boolean) => {
    if (state) change({ expanded: { ...state.ui.expanded, [id]: !open } });
  };
  const answer = (body: GateAnswer) => action.run("/gate", body);
  return (
    <>
      <LiveHeader
        state={state}
        loadError={error}
        busy={action.busy}
        follow={(runId) => void action.run("/follow", { runId })}
        cancel={() => void action.run("/cancel")}
      />
      <main id="main" className="page-main">
        <Notices>
          {action.error && <Notice tone="error" title={action.error} />}
          {error && <Notice tone="error" title={error} />}
        </Notices>
        {state && <GateList gates={state.gates} answer={answer} />}
        <RunTree state={state} loadError={error} toggle={toggle} />
      </main>
    </>
  );
}