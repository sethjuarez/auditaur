// One status as an octicon in its role color, with the status as its accessible name.
import { AlertIcon, CheckCircleFillIcon, CircleIcon, ClockIcon, SkipIcon, StopIcon, XCircleFillIcon, type Icon } from "@primer/octicons-react";
import { Spinner } from "@primer/react";
import { STATUS_LABELS, statusOf } from "../lib/status";
import "./StatusIcon.css";

const ICONS: Record<string, Icon> = {
  passed: CheckCircleFillIcon,
  failed: XCircleFillIcon,
  timedOut: ClockIcon,
  flaky: AlertIcon,
  interrupted: StopIcon,
  skipped: SkipIcon,
  notRun: CircleIcon,
  pending: CircleIcon,
};

export function StatusIcon({ status, label }: { status: unknown; label?: string }) {
  const value = statusOf(status);
  const name = label ?? STATUS_LABELS[value];
  if (value === "running") {
    return (
      <span className="status-icon running" title={name} role="img" aria-label={name}>
        <Spinner size="small" srText={null} />
      </span>
    );
  }
  const Glyph = ICONS[value];
  return (
    <span className={`status-icon ${value}`} title={name} role="img" aria-label={name}>
      <Glyph size={16} />
    </span>
  );
}
