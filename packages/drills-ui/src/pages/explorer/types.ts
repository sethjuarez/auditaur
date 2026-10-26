// The explorer's `/state`, as canvases/explorer/view.mjs builds it.
import type { HistoryEntry } from "../../components/HistoryDots";

export interface ExplorerTest {
  id: string;
  name: string;
  title: string;
  tags: string[];
  mode?: string;
  modeReason?: string;
  slow?: boolean;
  steps: { title: string }[];
  last: { runId: string; result: string; durationMs?: number; error?: string } | null;
  history: HistoryEntry[];
  flaky?: boolean;
  shown: boolean;
}

export interface ExplorerSuite {
  suite: string;
  path: string;
  description?: string;
  tags: string[];
  tests: ExplorerTest[];
}

export interface ExplorerRun {
  runId: string;
  status: string;
  totals?: { passed?: number; tests?: number; flaky?: number };
  durationMs?: number;
  startedAt?: string;
}

export interface ExplorerState {
  listError?: string | null;
  invalid: { path: string; errors?: { message?: string }[] }[];
  suites: ExplorerSuite[];
  runs: ExplorerRun[];
  active: { runId: string; done: number; total: number; current?: string } | null;
  launch: { exited: boolean; exitCode: number | null; output?: string } | null;
  ui: { filter: string; expanded: string[] };
}

export const isRunning = (state: ExplorerState | null) => Boolean(state?.active) || Boolean(state?.launch && !state.launch.exited);
