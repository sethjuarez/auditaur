// App-wide warnings: stale extension code, a CLI that doesn't match the plugin, and pages whose
// server side failed to load.
import { Notice, Notices } from "../components/Notice";
import { pageMeta } from "./pageMeta";
import type { Health } from "./useHealth";

// How to line the canvas up with the CLI, by how the canvas was installed.
function SkewFix({ source }: { source: NonNullable<Health["skew"]>["source"] }) {
  if (source === "plugin") {
    return (
      <>
        Install the matching CLI, or set <code>AUDITAUR_BIN</code> to it.
      </>
    );
  }
  return (
    <>
      Run <code>auditaur init extension --force</code>
      {source === "bundled" ? " (or reinstall the extension from the CLI's release tag)" : ""}, then reload extensions
      or restart the session.
    </>
  );
}

export function HealthNotices({ health }: { health: Health | null }) {
  if (!health || (!health.stale && !health.skew && !health.failedPages.length)) return null;
  return (
    <div className="app-notices">
      <Notices>
        {health.stale && (
          <Notice tone="warning" title="The drills extension changed on disk">
            This panel still runs the old code
            {health.onDiskVersion && health.onDiskVersion !== health.version
              ? ` (${health.version ?? "unknown"}; ${health.onDiskVersion} is on disk)`
              : ""}
            . Reopening the canvas reuses the same process, so reload extensions (or restart the session) to pick
            up the change.
          </Notice>
        )}
        {health.skew && (
          <Notice tone="warning" title="The drills canvas doesn't match the Auditaur CLI">
            The canvas is {health.skew.canvas} but the CLI is {health.skew.cli}. <SkewFix source={health.skew.source} />
          </Notice>
        )}
        {health.failedPages.map(({ page, error }) => (
          <Notice key={page} tone="error" title={`The ${pageMeta(page)?.label ?? page} page failed to load`} pre>{error}</Notice>
        ))}
      </Notices>
    </div>
  );
}