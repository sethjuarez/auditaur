// One snapshot result: where it came from, its status and baseline, how to act on it, then the
// expected/actual comparison.
import type { CompareMode } from "../../components/ImageCompare";
import { AriaResult } from "./AriaResult";
import { ScreenshotResult } from "./ScreenshotResult";
import { SnapshotCardHeader } from "./SnapshotCardHeader";
import { StatusHint } from "./StatusHint";
import type { SnapshotItem } from "./types";
import "../../components/Box.css";

export interface SplitControl {
  value: number;
  live: (split: number) => void;
  // A drag that ended without a release (the slider unmounted).
  cancel: () => void;
  commit: (split: number) => void;
}

interface Props {
  item: SnapshotItem;
  runId: string;
  running: boolean;
  locked: boolean;
  mode: CompareMode;
  split: SplitControl;
  accept: (ids: string[]) => void;
}

export function SnapshotCard({ item, runId, running, locked, mode, split, accept }: Props) {
  return (
    <section className="snap box" aria-label={item.step}>
      <SnapshotCardHeader item={item} running={running} locked={locked} accept={() => accept([item.id])} />
      <div className="snap-body">
        <StatusHint item={item} />
        {item.kind === "screenshot"
          ? <ScreenshotResult item={item} runId={runId} mode={mode} split={split} />
          : <AriaResult item={item} runId={runId} />}
      </div>
    </section>
  );
}
