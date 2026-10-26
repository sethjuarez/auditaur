// Trace: step through one drill test attempt with screenshots, DOM, and telemetry lined up by time.
// Follows the newest run unless one is picked; the shown step and kinds are the server's view.
import { Notice, Notices } from "../../components/Notice";
import { useAction } from "../../hooks/useAction";
import { useCanvasState } from "../../hooks/useCanvasState";
import { TraceBody } from "./TraceBody";
import { TraceHeader } from "./TraceHeader";
import type { SelectBody, TraceState } from "./types";
import "./trace.css";

export function TracePage() {
  const { state, error, reload, change } = useCanvasState<TraceState>((current) => (current?.run?.status === "running" ? 2000 : null));
  const action = useAction(reload);
  const select = (body: SelectBody) => void action.run("/select", body);
  const toggleKind = (kind: string) => {
    if (!state) return;
    const kinds = state.ui.kinds;
    change({ kinds: kinds.includes(kind) ? kinds.filter((each) => each !== kind) : [...kinds, kind] });
  };
  const pick = (step: number | "all") => {
    if (state?.selected) change({ step });
  };
  return (
    <>
      <TraceHeader state={state} loadError={error} busy={action.busy} select={select} reload={() => void reload()} />
      <main id="main" className="page-main trace-main">
        <Notices>
          {action.error && <Notice tone="error" title={action.error} />}
          {error && <Notice tone="error" title={error} />}
        </Notices>
        <TraceBody state={state} loadError={error} pick={pick} toggleKind={toggleKind} />
      </main>
    </>
  );
}
