// Entry point: the server's page shell names its page in `#root[data-page]`.
import { BaseStyles } from "@primer/react";
import { StrictMode, type ComponentType } from "react";
import { createRoot } from "react-dom/client";
import { Toast } from "./components/Toast";
import { useColorMode } from "./hooks/useColorMode";
import { ExplorerPage } from "./pages/explorer/ExplorerPage";
import "./theme/base.css";
import "./theme/bridge.css";

const PAGES: Record<string, { title: string; Page: ComponentType }> = {
  explorer: { title: "Drill tests", Page: ExplorerPage },
};

function App({ Page }: { Page: ComponentType }) {
  const mode = useColorMode();
  return (
    <BaseStyles className="drills-app" data-color-mode={mode} data-light-theme="light" data-dark-theme="dark">
      <Page />
      <Toast />
    </BaseStyles>
  );
}

const root = document.getElementById("root")!;
const entry = PAGES[root.dataset.page ?? ""];
if (entry) document.title = entry.title;
createRoot(root).render(
  <StrictMode>
    {entry ? <App Page={entry.Page} /> : <p>Unknown canvas page: {root.dataset.page}</p>}
  </StrictMode>,
);
