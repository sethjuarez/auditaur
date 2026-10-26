// The explorer's messages: run progress, failed actions or launches, and unreadable drill files.
import { ProgressBar } from "@primer/react";
import { Notice, Notices } from "../../components/Notice";
import type { ExplorerState } from "./types";

interface Props {
  state: ExplorerState | null;
  actionError: string | null;
  loadError: string | null;
}

export function ExplorerNotices({ state, actionError, loadError }: Props) {
  const active = state?.active;
  const launch = state?.launch;
  const pct = active?.total ? Math.round((100 * active.done) / active.total) : 0;
  return (
    <Notices>
      {active && (
        <Notice tone="info" className="run" title={<span className="nums">Running {active.done}/{active.total}{active.current ? ` · ${active.current}` : ""}</span>}>
          <ProgressBar progress={pct} barSize="small" aria-label="Run progress" className="run-progress" />
        </Notice>
      )}
      {!active && launch && !launch.exited && <Notice tone="info" className="run" title="Starting drill test…" />}
      {actionError && <Notice tone="error" title={actionError} />}
      {launch?.exited && launch.output && (
        <Notice tone="error" pre title={launch.exitCode == null ? "drill test failed to start" : `drill test exited with code ${launch.exitCode}`}>
          {launch.output}
        </Notice>
      )}
      {loadError && <Notice tone="error" title={loadError} />}
      {state?.listError && <Notice tone="error" title={state.listError} />}
      {state?.invalid.map((file) => (
        <Notice key={file.path} tone="error" title={<span className="path">{file.path}</span>}>
          {(file.errors ?? []).map((error) => error.message ?? JSON.stringify(error)).join("; ")}
        </Notice>
      ))}
    </Notices>
  );
}
