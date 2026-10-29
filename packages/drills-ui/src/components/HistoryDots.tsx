// A test's recent results as small squares, oldest first, so the newest sits at the right edge.
import { STATUS_LABELS, statusOf } from "../lib/status";
import "./HistoryDots.css";

export interface HistoryEntry { runId: string; result: string }

export function HistoryDots({ history }: { history: HistoryEntry[] }) {
  if (!history.length) return <span className="history" />;
  return (
    <span className="history" aria-label="Recent results, oldest first">
      {history.map((entry) => {
        const status = statusOf(entry.result);
        return <span key={entry.runId} className={`dot ${status}`} title={`${entry.runId}: ${STATUS_LABELS[status]}`} />;
      })}
    </span>
  );
}
