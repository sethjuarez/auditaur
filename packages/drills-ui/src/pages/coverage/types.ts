// The coverage canvas's `/state`, as canvases/coverage/view.mjs builds it from `drill coverage --json`.
export interface CommandCoverage {
  command: string;
  calls: number;
  failed: number;
  tests: string[];
  seen: number;
  // Listed by the app's registered commands (`generate_handler!` or `--commands`).
  registered?: boolean;
}

export interface EventCoverage {
  event: string;
  directions: string[];
  count: number;
  tests: string[];
  seen: number;
}

export interface TestCoverage {
  id: string;
  title: string;
  status: string;
  commands: string[];
  events: string[];
  telemetry: "recorded" | "none" | "unreadable" | "partial";
  error?: string;
}

export interface Coverage {
  runId: string;
  totals: { commands: number; commandsExercised: number; events: number; eventsExercised: number; tests: number; testsWithTelemetry: number };
  commands: CommandCoverage[];
  events: EventCoverage[];
  tests: TestCoverage[];
  sources: { db: string; ipcCalls: number; events: number; error?: string }[];
  // Where the registered commands were read from (absent from CLIs before 0.4.21).
  surface?: { path: string; commands: number; error?: string }[];
  baseline?: { commands: boolean; events: boolean };
}

export type CoverageTab = "commands" | "events" | "tests" | "map";

// Without a baseline (a readable store, or for commands the registered list), counts are out of
// only what the run touched.
const storeBaseline = (coverage: Coverage) => coverage.sources.some((source) => !source.error);
export const baselineFor = (coverage: Coverage, kind: "commands" | "events") =>
  coverage.baseline?.[kind] ?? storeBaseline(coverage);
export const hasRegistry = (coverage: Coverage) => (coverage.surface ?? []).some((surface) => !surface.error);

export interface CoverageState {
  pinned: string | null;
  runs: { runId: string; status: string }[];
  dbs: string[];
  // `apps`: the service names the run's suites launched.
  run: { runId: string; status: string; apps?: string[] } | null;
  coverage: Coverage | null;
  error: string | null;
  ui: { tab: CoverageTab; missedOnly: boolean };
}

// What "unexercised only" keeps: items no test touched, and tests that touched nothing.
export const shownCommands = (coverage: Coverage, missedOnly: boolean) =>
  missedOnly ? coverage.commands.filter((item) => !item.tests.length) : coverage.commands;
export const shownEvents = (coverage: Coverage, missedOnly: boolean) =>
  missedOnly ? coverage.events.filter((item) => !item.tests.length) : coverage.events;
export const shownTests = (coverage: Coverage, missedOnly: boolean) =>
  missedOnly ? coverage.tests.filter((test) => !test.commands.length && !test.events.length) : coverage.tests;
