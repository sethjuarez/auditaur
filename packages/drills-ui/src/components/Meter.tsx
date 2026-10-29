// A labelled "n of m" count with a proportional bar. `unit` completes the spoken value, e.g.
// "commands exercised" reads as "12 of 20 commands exercised".
import "./Meter.css";

interface Props {
  label: string;
  value: number;
  max: number;
  unit: string;
}

export function Meter({ label, value, max, unit }: Props) {
  const pct = max ? Math.round((value / max) * 100) : 0;
  const tone = !max ? "empty" : value >= max ? "complete" : "partial";
  // A 0..0 range isn't a valid meter, so an empty count is plain labelled text.
  const range = max
    ? { role: "meter", "aria-valuemin": 0, "aria-valuemax": max, "aria-valuenow": value, "aria-valuetext": `${value} of ${max} ${unit}` }
    : { role: "group" };
  return (
    <div className={`meter ${tone}`} aria-label={max ? label : `${label}: 0 of 0 ${unit}`} {...range}>
      <div className="meter-label" aria-hidden="true">
        <span className="meter-name">{label}</span>
        <span className="meter-value muted nums">{`${value} of ${max}${max ? ` · ${pct}%` : ""}`}</span>
      </div>
      <div className="meter-track" aria-hidden="true">
        <div className="meter-fill" style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}
