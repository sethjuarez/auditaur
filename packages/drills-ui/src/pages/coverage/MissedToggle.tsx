// Narrows every list to what no test exercised.
import { Checkbox, FormControl } from "@primer/react";

export function MissedToggle({ checked, set }: { checked: boolean; set: (missedOnly: boolean) => void }) {
  return (
    <FormControl id="missed" className="coverage-missed">
      <Checkbox checked={checked} onChange={(event) => set(event.target.checked)} />
      <FormControl.Label>Only show unexercised</FormControl.Label>
    </FormControl>
  );
}
