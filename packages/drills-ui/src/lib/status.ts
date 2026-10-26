// Test, step, and run statuses as the drill runner reports them, and how each one reads.
export type Status = "passed" | "failed" | "timedOut" | "flaky" | "interrupted" | "running" | "skipped" | "notRun" | "pending";

export const STATUS_LABELS: Record<Status, string> = {
  passed: "passed",
  failed: "failed",
  timedOut: "timed out",
  flaky: "flaky",
  interrupted: "interrupted",
  running: "running",
  skipped: "skipped",
  notRun: "not run",
  pending: "pending",
};

// Any unknown value reads as "not run" rather than breaking the page.
export const statusOf = (value: unknown): Status => (typeof value === "string" && value in STATUS_LABELS ? (value as Status) : "notRun");
