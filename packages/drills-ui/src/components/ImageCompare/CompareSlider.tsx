// Expected laid over actual, split by a draggable divider. The range input is the accessible
// slider (and what the agent's `split` drives); the pointer drags anywhere on the images.
import { GrabberIcon } from "@primer/octicons-react";
import { useEffect, useRef, type CSSProperties, type KeyboardEvent } from "react";
import { useCoveredLabels } from "./useCoveredLabels";
import { splitForKey, useSplitDrag } from "./useSplitDrag";

interface Props {
  expected: string;
  actual: string;
  // Percent of the width showing expected.
  split: number;
  onLive: (split: number) => void;
  onCommit: (split: number) => void;
  onCancel: () => void;
}

export function CompareSlider({ expected, actual, split, onLive, onCommit, onCancel }: Props) {
  const root = useRef<HTMLDivElement>(null);
  const range = useRef<HTMLInputElement>(null);
  const commit = useRef(onCommit);
  commit.current = onCommit;
  const labels = useCoveredLabels(root, split);
  const drag = useSplitDrag(onLive, onCommit, () => range.current?.focus({ preventScroll: true }), onCancel);

  // React's onChange fires on every `input`; the native `change` marks a finished adjustment.
  useEffect(() => {
    const input = range.current;
    if (!input) return;
    const changed = () => commit.current(Number(input.value));
    input.addEventListener("change", changed);
    return () => input.removeEventListener("change", changed);
  }, []);

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    const next = splitForKey(event.key, event.shiftKey, split);
    if (next == null) return;
    event.preventDefault();
    if (next !== split) onCommit(next);
  };

  return (
    <div ref={root} className="compare-slider" style={{ "--split": `${split}%` } as CSSProperties} {...drag}>
      <img className="compare-bottom" alt="Actual screenshot" src={actual} draggable={false} />
      <img className="compare-top" alt="Expected screenshot" src={expected} draggable={false} />
      <span ref={labels.left} className={`compare-label left${labels.covered.left ? " covered" : ""}`} aria-hidden="true">Expected</span>
      <span ref={labels.right} className={`compare-label right${labels.covered.right ? " covered" : ""}`} aria-hidden="true">Actual</span>
      <input
        ref={range}
        type="range"
        className="compare-range"
        min={0}
        max={100}
        step={1}
        value={split}
        aria-label="Expected and actual split"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={split}
        aria-valuetext={`${split}% expected, ${100 - split}% actual`}
        onChange={(event) => onLive(Number(event.target.value))}
        onKeyDown={onKeyDown}
      />
      <span className="compare-handle" aria-hidden="true">
        <span className="compare-knob"><GrabberIcon size={16} /></span>
      </span>
    </div>
  );
}
