// App-wide warnings: stale extension code, and pages whose server side failed to load.
import { Notice, Notices } from "../components/Notice";
import { pageMeta } from "./pageMeta";
import type { Health } from "./useHealth";

export function HealthNotices({ health }: { health: Health | null }) {
  if (!health || (!health.stale && !health.failedPages.length)) return null;
  return (
    <div className="app-notices">
      <Notices>
        {health.stale && (
          <Notice tone="warning" title="The drills extension changed on disk">
            This panel still runs the old code. Reload extensions to pick up the change.
          </Notice>
        )}
        {health.failedPages.map(({ page, error }) => (
          <Notice key={page} tone="error" title={`The ${pageMeta(page)?.label ?? page} page failed to load`} pre>{error}</Notice>
        ))}
      </Notices>
    </div>
  );
}