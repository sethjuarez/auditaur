// Which page's API a component talks to: `/api/<page>`. Pages, and the hooks inside them, read it
// so the same components work on any page.
import { createContext, useContext, type ReactNode } from "react";
import type { PageId } from "./route";

const PageApi = createContext<string | null>(null);

export function PageApiProvider({ page, children }: { page: PageId; children: ReactNode }) {
  return <PageApi.Provider value={`/api/${page}`}>{children}</PageApi.Provider>;
}

export function usePageApi(): string {
  const base = useContext(PageApi);
  if (!base) throw new Error("usePageApi outside a page");
  return base;
}

// A page-relative path (`/state`) under the page's API; app-wide `/api/...` paths stay as they are.
export const apiPath = (base: string, path: string) => (path.startsWith("/api/") ? path : `${base}${path}`);