// The snapshots canvas's `/state`, as canvases/snapshots/view.mjs builds it.
import type { CompareMode } from "../../components/ImageCompare";
import type { Status } from "../../lib/status";

export interface SnapshotFile {
  path: string;
  contentType: string;
}

export type SnapshotStatus = "changed" | "missing" | "accepted" | "noActual" | "unstable" | "unreadable";

export interface SnapshotItem {
  id: string;
  suite: string;
  test: string;
  stepIndex: number;
  step: string;
  kind: "aria" | "screenshot";
  baseline: string;
  stable: boolean;
  message: string;
  files: { expected?: SnapshotFile; actual?: SnapshotFile; diff?: SnapshotFile };
  status: SnapshotStatus;
}

export interface SnapshotsState {
  workspace: string;
  runs: { runId: string; status: string }[];
  pinned: string | null;
  run: { runId: string; status: string; startedAt?: string } | null;
  items: SnapshotItem[];
  ui: { mode: CompareMode; split: number };
}

// How each result reads, as a drill status for its icon and color.
export const STATUS: Record<SnapshotStatus, { status: Status; label: string; hint: string | null }> = {
  changed: { status: "failed", label: "Changed", hint: null },
  missing: { status: "failed", label: "No baseline", hint: null },
  accepted: { status: "passed", label: "Accepted", hint: "The baseline already matches this run's output. Re-run the drills to confirm." },
  noActual: { status: "skipped", label: "No actual output", hint: "The run saved no output for this step, so there is nothing to accept." },
  unstable: { status: "flaky", label: "Never stabilized", hint: "The output kept changing while the step waited for it to settle. Make the step deterministic, then re-run." },
  unreadable: { status: "failed", label: "Baseline unreadable", hint: "The baseline file could not be read. Fix or delete it, then re-run." },
};

export const statusInfo = (item: SnapshotItem) => STATUS[item.status] ?? STATUS.changed;

// Only results whose actual output can replace the baseline.
export const isAcceptable = (item: SnapshotItem) => item.status === "changed" || item.status === "missing";

export const KIND_LABELS: Record<SnapshotItem["kind"], string> = { aria: "ARIA snapshot", screenshot: "Screenshot" };
