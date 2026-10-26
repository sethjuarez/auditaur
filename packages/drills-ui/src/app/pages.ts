// The component for every page. Adding a page means: a server feature
// (.github/extensions/auditaur-drills/features/<id>), its label in pageMeta.ts, its route params
// in route.ts, and its component here.
import type { ComponentType } from "react";
import { CoveragePage } from "../pages/coverage/CoveragePage";
import { ExplorerPage } from "../pages/explorer/ExplorerPage";
import { GatePage } from "../pages/gate/GatePage";
import { LivePage } from "../pages/live/LivePage";
import { SnapshotsPage } from "../pages/snapshots/SnapshotsPage";
import { TracePage } from "../pages/trace/TracePage";
import { PAGE_META, type PageMeta } from "./pageMeta";
import type { PageId } from "./route";

const COMPONENTS: Record<PageId, ComponentType> = {
  tests: ExplorerPage,
  run: LivePage,
  trace: TracePage,
  snapshots: SnapshotsPage,
  coverage: CoveragePage,
  gate: GatePage,
};

export const PAGES: (PageMeta & { Page: ComponentType })[] = PAGE_META.map((meta) => ({ ...meta, Page: COMPONENTS[meta.id] }));