// How image comparisons show: side by side, an overlay slider, or the pixel diff.
import { SegmentedControl } from "@primer/react";
import { COMPARE_MODES, type CompareMode } from "./modes";

export function CompareModeControl({ mode, pick }: { mode: CompareMode; pick: (mode: CompareMode) => void }) {
  return (
    <SegmentedControl size="small" aria-label="Compare screenshots" onChange={(index) => pick(COMPARE_MODES[index].mode)}>
      {COMPARE_MODES.map((each) => (
        <SegmentedControl.Button key={each.mode} data-compare-mode={each.mode} selected={each.mode === mode} leadingVisual={each.icon} title={each.hint}>
          {each.label}
        </SegmentedControl.Button>
      ))}
    </SegmentedControl>
  );
}
