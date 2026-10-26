// The app's tabs. Each returns to where the user last was on that page; the button on the right
// opens the current page in a new panel.
import { LinkExternalIcon } from "@primer/octicons-react";
import { IconButton, UnderlineNav } from "@primer/react";
import { PAGE_META, type PageMeta } from "./pageMeta";
import { routeFor, type Route } from "./route";
import { navigate, openInNewPanel, useRoute } from "./routeStore";

const hasTab = (page: PageMeta, route: Route) => !page.optional || route.page === page.id || route.recent[page.id] != null;

export function AppNav({ build }: { build: string | null }) {
  const route = useRoute();
  return (
    <div className="app-nav">
      <UnderlineNav aria-label="Drills" className="app-nav-tabs">
        {PAGE_META.filter((page) => hasTab(page, route)).map((page) => {
          const href = route.recent[page.id] ?? routeFor(page.id);
          return (
            <UnderlineNav.Item
              key={page.id}
              href={href}
              data-page={page.id}
              icon={page.icon}
              title={page.purpose}
              aria-current={route.page === page.id ? "page" : undefined}
              onSelect={(event) => {
                event.preventDefault();
                void navigate(href);
              }}
            >
              {page.label}
            </UnderlineNav.Item>
          );
        })}
      </UnderlineNav>
      {build && <span className="app-build muted mono" title="Extension build">{build}</span>}
      <IconButton
        icon={LinkExternalIcon}
        size="small"
        variant="invisible"
        aria-label="Open this page in a new panel"
        id="open-new-panel"
        onClick={() => void openInNewPanel(route.path)}
      />
    </div>
  );
}