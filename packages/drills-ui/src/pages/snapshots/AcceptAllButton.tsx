// "Accept all" for every result that can take a new baseline, after confirming which baseline
// files it overwrites.
import { CheckIcon } from "@primer/octicons-react";
import { Button, ConfirmationDialog } from "@primer/react";
import { useState } from "react";
import type { SnapshotItem } from "./types";

interface Props {
  items: SnapshotItem[];
  running: boolean;
  locked: boolean;
  accept: (ids: string[]) => void;
}

export function AcceptAllButton({ items, running, locked, accept }: Props) {
  const [confirming, setConfirming] = useState(false);
  const count = items.length;
  const close = (gesture: string) => {
    setConfirming(false);
    if (gesture === "confirm") accept(items.map((item) => item.id));
  };
  return (
    <>
      <Button
        id="accept-all"
        size="small"
        variant="primary"
        leadingVisual={CheckIcon}
        disabled={locked}
        title={running ? "Wait for the run to finish" : "Copy this run's actual output over every changed or missing baseline"}
        onClick={() => setConfirming(true)}
      >
        {`Accept all as new baselines (${count})`}
      </Button>
      {confirming && (
        <ConfirmationDialog
          title={count === 1 ? "Accept 1 new baseline?" : `Accept ${count} new baselines?`}
          confirmButtonContent={count === 1 ? "Accept baseline" : `Accept ${count} baselines`}
          confirmButtonType="primary"
          overrideButtonFocus="cancel"
          onClose={close}
          width="large"
        >
          <p className="accept-all-lede">This run's actual output overwrites these baseline files:</p>
          <ul className="accept-all-files">
            {items.map((item) => <li key={item.id} className="path">{item.baseline}</li>)}
          </ul>
        </ConfirmationDialog>
      )}
    </>
  );
}
