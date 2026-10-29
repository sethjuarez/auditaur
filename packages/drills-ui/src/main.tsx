// Entry point: one routed app. The route comes from the server, so it survives reloads and the
// agent's `navigate` and the user's clicks are the same thing.
import { BaseStyles } from "@primer/react";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { AppShell } from "./app/AppShell";
import { startRouteStore } from "./app/routeStore";
import { Toast } from "./components/Toast";
import { useColorMode } from "./hooks/useColorMode";
import { message } from "./lib/api";
import "./theme/base.css";

function App() {
  const mode = useColorMode();
  return (
    <BaseStyles className="drills-app" data-color-mode={mode} data-light-theme="light" data-dark-theme="dark">
      <AppShell />
      <Toast />
      {/* Dialogs and overlays render here, inside the themed app, instead of at the end of body. */}
      <div data-portal-root />
    </BaseStyles>
  );
}

const root = createRoot(document.getElementById("root")!);
startRouteStore().then(
  () => root.render(<StrictMode><App /></StrictMode>),
  (err) => root.render(<p role="alert">Drills canvas server unreachable: {message(err)}</p>),
);