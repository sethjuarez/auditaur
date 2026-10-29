// Copies one result's actual output over its baseline.
import { CheckIcon } from "@primer/octicons-react";
import { Button } from "@primer/react";

interface Props {
  id: string;
  running: boolean;
  locked: boolean;
  accept: () => void;
}

export function AcceptButton({ id, running, locked, accept }: Props) {
  return (
    <Button
      size="small"
      className="snap-accept"
      data-accept={id}
      leadingVisual={CheckIcon}
      disabled={locked}
      title={running ? "Wait for the run to finish" : "Copy this run's actual output over the baseline file"}
      onClick={accept}
    >
      Accept as new baseline
    </Button>
  );
}
