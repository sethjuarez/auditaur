// A count with nothing to measure it against: what the run touched, with no percentage or bar,
// because "5 of 5" would claim full coverage of a surface nobody listed.
import "../../components/Meter.css";

interface Props {
  label: string;
  value: number;
  unit: string;
}

export function TouchedCount({ label, value, unit }: Props) {
  return (
    <div className="meter empty" role="group" aria-label={`${label}: ${value} ${unit} touched, no baseline`}>
      <div className="meter-label" aria-hidden="true">
        <span className="meter-name">{label}</span>
        <span className="meter-value muted nums">{`${value} touched`}</span>
      </div>
    </div>
  );
}
