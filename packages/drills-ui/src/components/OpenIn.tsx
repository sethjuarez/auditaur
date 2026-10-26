// "Open in" links from one drill canvas to another. Each posts the panel's `/open`
// (lib/navigator.mjs), which opens or focuses the target canvas on the same run or test.
import { ArrowUpRightIcon } from "@primer/octicons-react";
import { Button } from "@primer/react";
import { useState, type MouseEvent } from "react";
import { message, postJson } from "../lib/api";
import { showToast } from "../lib/toast";
import "./OpenIn.css";

export type CanvasName = "run" | "trace" | "snapshots" | "coverage";
export interface OpenInLink { to: CanvasName; runId?: string; testId?: string; label?: string }

const LABELS: Record<CanvasName, string> = { run: "Run", trace: "Trace", snapshots: "Snapshots", coverage: "Coverage" };

// Links to every other run canvas for `runId`, from the page `here`.
export const runLinks = (runId: string | null | undefined, here: CanvasName | null): OpenInLink[] =>
  runId ? (Object.keys(LABELS) as CanvasName[]).filter((to) => to !== here).map((to) => ({ to, runId })) : [];

function OpenInButton({ link }: { link: OpenInLink }) {
  const [busy, setBusy] = useState(false);
  // A link inside a clickable row opens the canvas without also toggling the row.
  const open = async (event: MouseEvent) => {
    event.stopPropagation();
    setBusy(true);
    try {
      await postJson("/open", { to: link.to, ...(link.runId && { runId: link.runId }), ...(link.testId && { testId: link.testId }) });
    } catch (err) {
      showToast(`Could not open ${LABELS[link.to].toLowerCase()}: ${message(err)}`);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Button
      size="small"
      variant="invisible"
      className="open-in-link"
      data-open-to={link.to}
      data-open-run={link.runId}
      data-open-test={link.testId}
      title={`Open the ${LABELS[link.to].toLowerCase()} canvas${link.runId ? ` for ${link.runId}` : ""}`}
      trailingVisual={ArrowUpRightIcon}
      disabled={busy}
      onClick={open}
    >
      {link.label ?? LABELS[link.to]}
    </Button>
  );
}

export function OpenInLinks({ links }: { links: OpenInLink[] }) {
  if (!links.length) return null;
  return (
    <span className="open-in" role="group" aria-label="Open in">
      {links.map((link) => <OpenInButton key={`${link.to}:${link.runId ?? ""}:${link.testId ?? ""}`} link={link} />)}
    </span>
  );
}
