// Snapshots: review a drill run's ARIA and screenshot mismatches and accept new baselines.
// Follows the newest run unless one is picked; the compare mode and slider split are the server's view.
import { useState } from "react";
import { Notice, Notices } from "../../components/Notice";
import { useAction } from "../../hooks/useAction";
import { useCanvasState } from "../../hooks/useCanvasState";
import { plural } from "../../lib/format";
import { SnapshotsBody } from "./SnapshotsBody";
import { SnapshotsHeader } from "./SnapshotsHeader";
import type { SnapshotsState } from "./types";
import "./snapshots.css";

export function SnapshotsPage() {
  const { state, error, reload, change } = useCanvasState<SnapshotsState>((current) => (current?.run?.status === "running" ? 2000 : null));
  const action = useAction(reload);
  const [notice, setNotice] = useState<string | null>(null);
  // A slider being dragged: every slider shows it, and reloads can't snap it back, until release.
  const [liveSplit, setLiveSplit] = useState<number | null>(null);
  const locked = action.busy || state?.run?.status === "running";

  const select = async (runId: string | null) => {
    if (await action.run("/select", { runId })) setNotice(null);
  };
  const accept = (ids: string[]) => {
    if (!state?.run || !ids.length || locked) return;
    setNotice(null);
    void action.run<{ accepted: unknown[] }>("/accept", { runId: state.run.runId, ids }, (result) => {
      setNotice(`Accepted ${plural(result.accepted.length, "baseline")}. Re-run the drills to confirm.`);
    });
  };
  const split = {
    value: liveSplit ?? state?.ui.split ?? 50,
    live: setLiveSplit,
    cancel: () => setLiveSplit(null),
    commit: (value: number) => {
      setLiveSplit(null);
      if (value !== state?.ui.split) change({ split: value });
    },
  };

  return (
    <>
      <SnapshotsHeader
        state={state}
        loadError={error}
        busy={action.busy}
        locked={locked}
        select={(runId) => void select(runId)}
        reload={() => {
          action.clearError();
          void reload();
        }}
        setMode={(mode) => change({ mode })}
        acceptAll={accept}
      />
      <main id="main" className="page-main snapshots-main">
        <Notices>
          {action.error && <Notice tone="error" title={action.error} />}
          {error && <Notice tone="error" title={error} />}
          {notice && <Notice tone="ok" title={notice} />}
        </Notices>
        <SnapshotsBody state={state} loadError={error} locked={locked} split={split} accept={accept} />
      </main>
    </>
  );
}
