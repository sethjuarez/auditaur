// The app: tabs, app-wide notices, and the routed page inside its own error boundary and API base.
import { Redirect, Route as Path, Router, Switch } from "wouter";
import { PageApiProvider } from "./PageApi";
import { PageBoundary } from "./PageBoundary";
import { AppNav } from "./AppNav";
import { HealthNotices } from "./HealthNotices";
import { PAGES } from "./pages";
import { pathOf, searchOf } from "./route";
import { navigate, useRoute } from "./routeStore";
import { useHealth } from "./useHealth";
import "./app.css";

// wouter reads and changes the server-owned route instead of the browser location.
function useServerLocation(): [string, (to: string) => void] {
  return [pathOf(useRoute().path), (to) => void navigate(to)];
}
const useServerSearch = () => searchOf(useRoute().path);

export function AppShell() {
  const health = useHealth();
  const route = useRoute();
  return (
    <Router hook={useServerLocation} searchHook={useServerSearch}>
      <AppNav build={health?.build ?? null} />
      <HealthNotices health={health} />
      <Switch>
        {PAGES.map(({ id, label, Page }) => (
          <Path key={id} path={`/${id}`}>
            <PageApiProvider page={id}>
              <PageBoundary key={id} label={label} resetKey={route.path}>
                <Page />
              </PageBoundary>
            </PageApiProvider>
          </Path>
        ))}
        <Path><Redirect to="/tests" /></Path>
      </Switch>
    </Router>
  );
}