// A screenshot result: the runner's message, then expected and actual in the page's compare mode.
import { ImageCompare, type CompareMode } from "../../components/ImageCompare";
import { fileUrl } from "../../lib/files";
import type { SplitControl } from "./SnapshotCard";
import type { SnapshotItem } from "./types";

interface Props {
  item: SnapshotItem;
  runId: string;
  mode: CompareMode;
  split: SplitControl;
}

export function ScreenshotResult({ item, runId, mode, split }: Props) {
  const url = (name: keyof SnapshotItem["files"]) => {
    const file = item.files[name];
    return file ? fileUrl(runId, file.path) : null;
  };
  return (
    <>
      {item.message && <p className="snap-message muted">{item.message}</p>}
      <ImageCompare
        urls={{ expected: url("expected"), actual: url("actual"), diff: url("diff") }}
        mode={mode}
        split={split.value}
        onSplitLive={split.live}
        onSplitCommit={split.commit}
        onSplitCancel={split.cancel}
      />
    </>
  );
}
