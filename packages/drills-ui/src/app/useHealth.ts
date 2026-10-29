// The extension's build and whether it changed on disk since it started (then extensions need a
// reload), plus any page that failed to load. Checked on load and every half minute.
import { useEffect, useState } from "react";
import { getJson } from "../lib/api";
import type { PageId } from "./route";
import { isStill } from "./serverEvents";

export interface Health {
  build: string;
  stale: boolean;
  skew?: { plugin: string; cli: string } | null;
  failedPages: { page: PageId; error: string }[];
}

const INTERVAL_MS = 30_000;

export function useHealth(): Health | null {
  const [health, setHealth] = useState<Health | null>(null);
  useEffect(() => {
    let alive = true;
    const check = () => getJson<Health>("/api/health").then((value) => alive && setHealth(value), () => {});
    void check();
    const timer = isStill() ? undefined : setInterval(check, INTERVAL_MS);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, []);
  return health;
}