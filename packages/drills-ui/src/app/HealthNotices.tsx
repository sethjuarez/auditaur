// App-wide warnings: stale extension code, a CLI that doesn't match the plugin, and pages whose
// server side failed to load.
import { Notice, Notices } from "../components/Notice";
import { pageMeta } from "./pageMeta";
import type { Health } from "./useHealth";

export function HealthNotices({ health }: { health: Health | null }) {
  if (!health || (!health.stale && !health.skew && !health.failedPages.length)) return null;
  return (
    <div className="app-notices">
      <Notices>
        {health.stale && (
          <Notice tone="warning" title="The drills extension changed on disk">
            This panel still runs the old code. Reopening the canvas reuses the same process, so reload extensions
            (or restart the session) to pick up the change.
          </Notice>
        )}
        {health.skew && (
          <Notice tone="warning" title="The drills canvas doesn't match the Auditaur CLI">
            The canvas is {health.skew.canvas} but the CLI is {health.skew.cli}.{" "}
            {health.skew.source === "init" ? (
              <>
                Run <code>auditaur init extension --force</code>, then reload extensions.
              </>
            ) : (
              <>
                Install the matching CLI, or set <code>AUDITAUR_BIN</code> to it.
              </>
            )}
          </Notice>
        )}
        {health.failedPages.map(({ page, error }) => (
          <Notice key={page} tone="error" title={`The ${pageMeta(page)?.label ?? page} page failed to load`} pre>{error}</Notice>
        ))}
      </Notices>
    </div>
  );
}