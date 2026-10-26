// Beside a page title when the canvas server can't be reached: an alert, never a spinner.
import { AlertIcon } from "@primer/octicons-react";
import "./StatusIcon.css";

export function LoadAlert() {
  return (
    <>
      <span className="status-icon error" role="img" aria-label="Could not load"><AlertIcon size={16} /></span>
      <span className="status-text error">unreachable</span>
    </>
  );
}
