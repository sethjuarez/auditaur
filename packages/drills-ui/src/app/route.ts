// The panel's route, owned by the server (lib/route.mjs): which page shows, for which run and test.
export type PageId = "tests" | "run" | "trace" | "snapshots" | "coverage" | "gate";

export interface Route {
  path: string;
  page: PageId;
  // Bumped by the server on every navigation, so an older event never undoes a newer one.
  rev: number;
  // The last route of each page, so its tab returns where the user left it.
  recent: Partial<Record<PageId, string>>;
}

export interface Selection {
  runId?: string | null;
  testId?: string | null;
  attempt?: number | null;
}

// Mirrors the server's formatRoute: parameters in page order, empty ones dropped.
const PARAMS: Record<PageId, (keyof typeof NAMES)[]> = {
  tests: [],
  run: ["runId"],
  trace: ["runId", "testId", "attempt"],
  snapshots: ["runId"],
  coverage: ["runId"],
  gate: [],
};
const NAMES = { runId: "run", testId: "test", attempt: "attempt" } as const;

export function routeFor(page: PageId, selection: Selection = {}): string {
  const query = new URLSearchParams();
  for (const key of PARAMS[page]) {
    const value = selection[key];
    if (value != null && value !== "") query.set(NAMES[key], String(value));
  }
  const search = query.toString();
  return `/${page}${search ? `?${search}` : ""}`;
}

export const pathOf = (route: string) => route.split("?")[0];
export const searchOf = (route: string) => route.split("?")[1] ?? "";