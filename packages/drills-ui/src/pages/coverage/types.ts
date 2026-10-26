// The coverage canvas's `/state`, as canvases/coverage/view.mjs builds it from `drill coverage --json`.
export interface CommandCoverage {
  command: string;
  calls: number;
  failed: number;
  tests: string[];
  seen: number;
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
}

export type CoverageTab = "commands" | "events" | "tests" | "map";

export interface CoverageState {
  pinned: string | null;
  runs: { runId: string; status: string }[];
  dbs: string[];
  run: { runId: string; status: string } | null;
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
