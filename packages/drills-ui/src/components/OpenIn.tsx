// "Open in" buttons from one drill canvas to another. Each posts the panel's `/open`
// (lib/navigator.mjs), which opens or focuses the target canvas on the same run or test.
import { ChecklistIcon, ImageIcon, ListOrderedIcon, PulseIcon, type Icon } from "@primer/octicons-react";
import { Button, ButtonGroup } from "@primer/react";
import { useId, useState, type MouseEvent } from "react";
import { message, postJson } from "../lib/api";
import { showToast } from "../lib/toast";
import "./OpenIn.css";

export type CanvasName = "run" | "trace" | "snapshots" | "coverage";
export interface OpenInLink { to: CanvasName; runId?: string; testId?: string; label?: string }

// "run" is the live progress canvas; it is labelled "Progress" so it never reads as "execute".
const CANVASES: Record<CanvasName, { label: string; icon: Icon; purpose: string }> = {
  run: { label: "Progress", icon: PulseIcon, purpose: "live progress, results, and manual gates" },
  trace: { label: "Trace", icon: ListOrderedIcon, purpose: "steps with screenshots, IPC, events, and logs" },
  snapshots: { label: "Snapshots", icon: ImageIcon, purpose: "snapshot mismatches to review and accept" },
  coverage: { label: "Coverage", icon: ChecklistIcon, purpose: "which commands and events tests exercised" },
};

// Buttons for every other run canvas for `runId`, from the page `here`.
export const runLinks = (runId: string | null | undefined, here: CanvasName | null): OpenInLink[] =>
  runId ? (Object.keys(CANVASES) as CanvasName[]).filter((to) => to !== here).map((to) => ({ to, runId })) : [];

function OpenInButton({ link }: { link: OpenInLink }) {
  const [busy, setBusy] = useState(false);
  const canvas = CANVASES[link.to];
  // A button inside a clickable row opens the canvas without also toggling the row.
  const open = async (event: MouseEvent) => {
    event.stopPropagation();
    setBusy(true);
    try {
      await postJson("/open", { to: link.to, ...(link.runId && { runId: link.runId }), ...(link.testId && { testId: link.testId }) });
    } catch (err) {
      showToast(`Could not open ${canvas.label.toLowerCase()}: ${message(err)}`);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Button
      size="small"
      className="open-in-link"
      data-open-to={link.to}
      data-open-run={link.runId}
      data-open-test={link.testId}
      title={`Open the ${canvas.label} panel: ${canvas.purpose}${link.runId ? ` (${link.runId})` : ""}`}
      leadingVisual={canvas.icon}
      disabled={busy}
      onClick={open}
    >
      {link.label ?? canvas.label}
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