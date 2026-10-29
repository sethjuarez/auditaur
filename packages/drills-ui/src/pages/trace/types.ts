// The trace canvas's `/state`, as canvases/trace/view.mjs builds it.

export interface TraceAttachment {
  name: string;
  contentType: string;
  path: string;
}

export interface TraceStep {
  index: number;
  phase?: string;
  title: string;
  action?: string;
  status: string;
  soft: boolean;
  offsetMs: number | null;
  durationMs: number | null;
  error: string | null;
  attachments: TraceAttachment[];
}

export interface TraceEvent {
  kind: string;
  name: string;
  status: string | null;
  source?: string | null;
  detail: string;
  offsetMs: number | null;
  durationMs: number | null;
  // The step the event happened during, or null when it came before the first step.
  step: number | null;
}

export interface TraceSelected {
  key: string;
  kind: "test" | "hooks";
  title: string;
  attempt: number | null;
  status: string;
  durationMs: number | null;
  error: string | null;
  missing: boolean;
  filterNotes: unknown;
  steps: TraceStep[];
  attachments: TraceAttachment[];
  events: TraceEvent[];
  // The step the server shows for `ui.step: "auto"`: the failing one, or null for the whole attempt.
  step: number | null;
}

export interface TraceEntry {
  key: string;
  kind: "test" | "hooks";
  suite: string;
  title: string;
  result: string;
  attempts: { attempt: number; status: string }[];
}

export interface TraceRun {
  runId: string;
  status: string;
  startedAt?: string;
}

export type StepView = number | "auto" | "all";

export interface TraceState {
  workspace: string;
  runs: { runId: string; status: string }[];
  pinned: string | null;
  run: TraceRun | null;
  entries: TraceEntry[];
  selected: TraceSelected | null;
  ui: { step: StepView; kinds: string[] };
}

export interface SelectBody {
  runId: string | null;
  testId?: string;
  attempt?: number;
}

export const KINDS = ["ipc", "event", "log", "error", "span"] as const;
export type Kind = (typeof KINDS)[number];

export const KIND_LABELS: Record<Kind, { label: string; noun: string }> = {
  ipc: { label: "IPC", noun: "IPC calls" },
  event: { label: "Events", noun: "Tauri events" },
  log: { label: "Logs", noun: "log records" },
  error: { label: "Errors", noun: "errors" },
  span: { label: "Spans", noun: "other spans" },
};

// Span events are filtered and counted with spans.
export const kindOf = (event: TraceEvent) => (event.kind === "span-event" ? "span" : event.kind);

// The step shown: mirrors the server's reading of `ui.step`, so a click shows before the reload.
export function shownStep(selected: TraceSelected, step: StepView): number | null {
  if (step === "auto") return selected.step;
  if (step === "all") return null;
  return step < selected.steps.length ? step : null;
}

export { fileUrl } from "../../lib/files";

export const isImage = (attachment: TraceAttachment) => attachment.contentType.startsWith("image/");
