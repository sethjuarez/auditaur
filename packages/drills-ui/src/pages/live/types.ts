// The run canvas's `/state`, as canvases/live/view.mjs builds it.

export interface LiveStep {
  phase?: string;
  title: string;
  status: string;
  soft?: boolean;
  startedAt?: string;
  durationMs?: number;
  error?: string | null;
}

export interface LiveAttempt {
  attempt: number;
  status: string;
  durationMs?: number;
  error?: string | null;
  steps: LiveStep[];
}

export interface LiveTest {
  id: string;
  name: string;
  title: string;
  tags: string[];
  result: string;
  durationMs?: number;
  attempts: LiveAttempt[];
}

export interface LiveSuite {
  id: string;
  suite: string;
  path: string;
  status: string;
  durationMs?: number;
  error?: string | null;
  hooks: LiveStep[];
  tests: LiveTest[];
}

export interface LiveRun {
  runId: string;
  status: string;
  startedAt?: string;
  durationMs?: number;
  suites: LiveSuite[];
}

export interface GateChoice { id: string; label: string; outcome: string }
export interface GateInput { id: string; label?: string; kind?: string; required?: boolean; sensitive?: boolean }

export interface Gate {
  requestId: string | null;
  test: string;
  attempt?: number;
  since: string;
  name: string;
  instructions: string;
  selector: string | null;
  expectText: string | null;
  timeoutMs: number | null;
  manualContinue: boolean;
  choices: GateChoice[];
  inputs: GateInput[];
  clipboard: { label: string; value?: string; sensitive?: boolean; redacted?: boolean; status: string } | null;
}

export interface LiveState {
  workspace: string;
  pinned: string | null;
  runs: { runId: string; status: string }[];
  run: LiveRun | null;
  gates: Gate[];
  // Open (true) or closed (false) overrides by test id or `hooks:<suite id>`.
  ui: { expanded: Record<string, boolean> };
}

export const isLive = (state: LiveState | null) => state?.run?.status === "running";

export const testsOf = (run: LiveRun) => run.suites.flatMap((suite) => suite.tests);

export const countResult = (tests: LiveTest[], result: string) => tests.filter((test) => test.result === result).length;

export const isDone = (test: LiveTest) => test.result !== "running" && test.result !== "notRun";

// A row's open state: the user's (or agent's) override, else open while it needs attention.
export const isOpen = (expanded: Record<string, boolean>, id: string, auto: boolean) =>
  Object.hasOwn(expanded, id) ? expanded[id] : auto;

// Results in the order the summary and bar list them.
export const RESULTS = ["passed", "flaky", "failed", "interrupted", "skipped"] as const;