// A snapshot card's header: status, the suite › test › step it came from, its kind and status
// labels, the baseline file, and "Accept as new baseline" when the result can take one.
import { Label } from "@primer/react";
import { StatusIcon } from "../../components/StatusIcon";
import { AcceptButton } from "./AcceptButton";
import { isAcceptable, KIND_LABELS, statusInfo, type SnapshotItem } from "./types";

interface Props {
  item: SnapshotItem;
  running: boolean;
  locked: boolean;
  accept: () => void;
}

const LABEL_VARIANTS = { failed: "danger", passed: "success", flaky: "attention", skipped: "secondary" } as const;

export function SnapshotCardHeader({ item, running, locked, accept }: Props) {
  const info = statusInfo(item);
  // Tests named "<suite> > <test>" already carry their suite.
  const suite = String(item.test ?? "").startsWith(`${item.suite} > `) ? null : item.suite;
  const where = [suite, item.test].filter(Boolean).join(" › ");
  return (
    <div className="box-header snap-head">
      <StatusIcon status={info.status} label={info.label} />
      <div className="snap-title">
        {where && <span className="snap-test muted">{where}</span>}
        <h2 className="snap-step">{item.step}</h2>
        <div className="snap-where">
          <Label size="small" variant="secondary">{KIND_LABELS[item.kind] ?? item.kind}</Label>
          <Label size="small" variant={LABEL_VARIANTS[info.status as keyof typeof LABEL_VARIANTS] ?? "secondary"}>{info.label}</Label>
          <span className="snap-baseline path muted" title="Baseline file">{item.baseline}</span>
        </div>
      </div>
      {isAcceptable(item) && <AcceptButton id={item.id} running={running} locked={locked} accept={accept} />}
    </div>
  );
}
