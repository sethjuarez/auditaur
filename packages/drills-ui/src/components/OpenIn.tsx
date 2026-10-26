// "Open in" buttons to another page on the same run or test. A click navigates this panel;
// Ctrl- or Shift-click opens the page in a new panel and leaves this one as it is.
import { Button, ButtonGroup } from "@primer/react";
import { useId, type MouseEvent } from "react";
import { pageMeta } from "../app/pageMeta";
import { routeFor, type PageId } from "../app/route";
import { navigate, openInNewPanel } from "../app/routeStore";
import "./OpenIn.css";

export type CanvasName = Exclude<PageId, "tests">;
export interface OpenInLink { to: CanvasName; runId?: string; testId?: string; label?: string }

const RUN_PAGES: CanvasName[] = ["run", "trace", "snapshots", "coverage"];

// Buttons for every other run page for `runId`, from the page `here`.
export const runLinks = (runId: string | null | undefined, here: CanvasName | null): OpenInLink[] =>
  runId ? RUN_PAGES.filter((to) => to !== here).map((to) => ({ to, runId })) : [];

function OpenInButton({ link }: { link: OpenInLink }) {
  const page = pageMeta(link.to)!;
  const path = routeFor(link.to, { runId: link.runId, testId: link.testId });
  // A button inside a clickable row opens the page without also toggling the row.
  const open = (event: MouseEvent) => {
    event.stopPropagation();
    void (event.ctrlKey || event.metaKey || event.shiftKey ? openInNewPanel(path) : navigate(path));
  };
  return (
    <Button
      size="small"
      className="open-in-link"
      data-open-to={link.to}
      data-open-run={link.runId}
      data-open-test={link.testId}
      title={`${page.label}: ${page.purpose}${link.runId ? ` (${link.runId})` : ""}. Ctrl-click for a new panel.`}
      leadingVisual={page.icon}
      onClick={open}
    >
      {link.label ?? page.label}
    </Button>
  );
}

// `caption` names what is being opened, e.g. "Open last run in".
export function OpenInLinks({ links, caption = "Open in" }: { links: OpenInLink[]; caption?: string }) {
  const captionId = useId();
  if (!links.length) return null;
  return (
    <span className="open-in">
      <span className="open-in-caption muted" id={captionId}>{caption}</span>
      <ButtonGroup role="group" aria-labelledby={captionId}>
        {links.map((link) => <OpenInButton key={`${link.to}:${link.runId ?? ""}:${link.testId ?? ""}`} link={link} />)}
      </ButtonGroup>
    </span>
  );
}