// What each page is called and what it is for, in tab order. Kept apart from the page components
// (pages.ts) so anything can link to a page without importing every page.
import { ChecklistIcon, ImageIcon, ListOrderedIcon, PersonIcon, PulseIcon, TasklistIcon, type Icon } from "@primer/octicons-react";
import type { PageId } from "./route";

export interface PageMeta {
  id: PageId;
  label: string;
  icon: Icon;
  purpose: string;
  // Has a tab only while on screen or visited: most sessions never use it.
  optional?: boolean;
}

// "run" is live progress; it is labelled "Progress" so it never reads as "execute".
export const PAGE_META: PageMeta[] = [
  { id: "tests", label: "Tests", icon: TasklistIcon, purpose: "suites and tests with their latest results" },
  { id: "run", label: "Progress", icon: PulseIcon, purpose: "live progress, results, and manual gates" },
  { id: "trace", label: "Trace", icon: ListOrderedIcon, purpose: "steps with screenshots, IPC, events, and logs" },
  { id: "snapshots", label: "Snapshots", icon: ImageIcon, purpose: "snapshot mismatches to review and accept" },
  { id: "coverage", label: "Coverage", icon: ChecklistIcon, purpose: "which commands and events tests exercised" },
  { id: "gate", label: "Gate", icon: PersonIcon, purpose: "a manual gate of an auditaur drill run", optional: true },
];

export const pageMeta = (id: PageId): PageMeta | undefined => PAGE_META.find((page) => page.id === id);