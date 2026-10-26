// The answers a gate offers, as submit buttons: the first is the main one, and failing or aborting
// ones read as destructive.
import { Button } from "@primer/react";
import type { GateChoice } from "./types";

const CONTINUE: GateChoice = { id: "", label: "Continue", outcome: "continue" };
const DESTRUCTIVE = new Set(["fail", "abort"]);

export function GateChoices({ choices, disabled }: { choices: GateChoice[]; disabled: boolean }) {
  const shown = choices.length ? choices : [CONTINUE];
  return (
    <div className="row gate-choices">
      {shown.map((choice, index) => (
        <Button
          key={choice.id || "continue"}
          type="submit"
          size="small"
          value={choice.id}
          data-outcome={choice.outcome}
          variant={DESTRUCTIVE.has(choice.outcome) ? "danger" : index === 0 ? "primary" : "default"}
          disabled={disabled}
        >
          {choice.label}
        </Button>
      ))}
    </div>
  );
}