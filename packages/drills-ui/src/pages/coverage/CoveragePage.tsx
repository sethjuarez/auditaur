// Coverage: which IPC commands and events a drill run's tests exercised, and which they missed.
// Follows the newest run unless one is picked; the tab and "unexercised only" are the server's view.
import { routeFor } from "../../app/route";
import { navigate } from "../../app/routeStore";
import { Notice, Notices } from "../../components/Notice";
import { useAction } from "../../hooks/useAction";
import { useCanvasState } from "../../hooks/useCanvasState";
import { CoverageBody } from "./CoverageBody";
import { CoverageHeader } from "./CoverageHeader";
import type { CoverageState } from "./types";
import "../../components/Box.css";
import "./coverage.css";

export function CoveragePage() {
  const { state, error, reload, change } = useCanvasState<CoverageState>((current) => (current?.run?.status === "running" ? 2000 : null));
  const action = useAction(reload);
  const broken = (state?.coverage?.sources ?? []).filter((source) => source.error);
  const unlisted = (state?.coverage?.surface ?? []).filter((surface) => surface.error);

  return (
    <>
      <CoverageHeader
        state={state}
        loadError={error}
        busy={action.busy}
        select={(runId) => void navigate(routeFor("coverage", { runId }))}
        reload={() => {
          action.clearError();
          void reload("?refresh");
        }}
        setTab={(tab) => change({ tab })}
        setMissedOnly={(missedOnly) => change({ missedOnly })}
        setStores={(dbs) => void action.run("/sources", { dbs })}
      />
      <main id="main" className="page-main coverage-main">
        <Notices>
          {action.error && <Notice tone="error" title={action.error} />}
          {error && <Notice tone="error" title={error} />}
          {state?.error && <Notice tone="error" title="Could not read coverage" pre>{state.error}</Notice>}
          {broken.map((source) => (
            <Notice key={source.db} tone="warning" title={`Could not read ${source.db}`}>{source.error}</Notice>
          ))}
          {unlisted.map((surface) => (
            <Notice key={surface.path} tone="warning" title={`Could not read the app's commands from ${surface.path}`}>{surface.error}</Notice>
          ))}
        </Notices>
        <CoverageBody state={state} loadError={error} />
      </main>
    </>
  );
}
