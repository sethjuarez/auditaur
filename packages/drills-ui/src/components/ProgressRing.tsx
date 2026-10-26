// A 16px ring that fills as work completes, e.g. tests done out of a run's total. With a label it
// is a progressbar for assistive tech; without one it is decoration beside text that says the same.
import "./ProgressRing.css";

const RADIUS = 6;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;

export function ProgressRing({ value, max, label }: { value: number; max: number; label?: string }) {
  const fraction = max > 0 ? Math.min(1, Math.max(0, value / max)) : 0;
  const a11y = label
    ? { role: "progressbar", "aria-label": label, "aria-valuemin": 0, "aria-valuemax": max, "aria-valuenow": value }
    : { "aria-hidden": true };
  return (
    <svg className="progress-ring" width={16} height={16} viewBox="0 0 16 16" {...a11y}>
      {label && <title>{label}</title>}
      <circle className="progress-ring-track" cx={8} cy={8} r={RADIUS} />
      <circle
        className="progress-ring-fill"
        cx={8}
        cy={8}
        r={RADIUS}
        strokeDasharray={CIRCUMFERENCE}
        strokeDashoffset={CIRCUMFERENCE * (1 - fraction)}
        transform="rotate(-90 8 8)"
      />
    </svg>
  );
}