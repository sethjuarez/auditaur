// An expected/actual image comparison in one of three modes, for a snapshot that may lack a
// baseline, an actual image, or a diff.
import { CompareFigure } from "./CompareFigure";
import { CompareSlider } from "./CompareSlider";
import type { CompareMode } from "./modes";
import "./ImageCompare.css";

export interface ImageUrls {
  expected: string | null;
  actual: string | null;
  diff: string | null;
}

interface Props {
  urls: ImageUrls;
  mode: CompareMode;
  split: number;
  onSplitLive: (split: number) => void;
  onSplitCommit: (split: number) => void;
  onSplitCancel: () => void;
}

export function ImageCompare({ urls, mode, split, onSplitLive, onSplitCommit, onSplitCancel }: Props) {
  const { expected, actual, diff } = urls;
  if (!actual) return <p className="compare-note">The run saved no actual screenshot.</p>;
  if (!expected) return <CompareFigure url={actual} caption="Actual (no baseline yet)" alt="Actual screenshot" />;
  if (mode === "diff") {
    return diff
      ? <CompareFigure url={diff} caption="Diff: changed pixels are highlighted" alt="Pixel diff between expected and actual" />
      : <p className="compare-note">The run saved no diff image.</p>;
  }
  if (mode === "slider") return <CompareSlider expected={expected} actual={actual} split={split} onLive={onSplitLive} onCommit={onSplitCommit} onCancel={onSplitCancel} />;
  return (
    <div className="compare-side">
      <CompareFigure url={expected} caption="Expected (baseline)" alt="Expected screenshot" />
      <CompareFigure url={actual} caption="Actual (this run)" alt="Actual screenshot" />
    </div>
  );
}
