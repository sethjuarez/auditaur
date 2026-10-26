// One field a gate asks for: text, multiline text, or a masked secret.
import { FormControl, Textarea, TextInput } from "@primer/react";
import type { GateInput } from "./types";

export function GateField({ input, missing, onEdit }: { input: GateInput; missing: boolean; onEdit: () => void }) {
  const multiline = input.kind === "multilineText" && !input.sensitive;
  const validation = missing ? "error" : undefined;
  return (
    <FormControl required={input.required}>
      <FormControl.Label>{input.label ?? input.id}</FormControl.Label>
      {multiline
        ? <Textarea name={input.id} block rows={3} autoComplete="off" validationStatus={validation} onInput={onEdit} />
        : <TextInput name={input.id} block type={input.sensitive ? "password" : "text"} autoComplete="off" validationStatus={validation} onInput={onEdit} />}
      {missing && <FormControl.Validation variant="error">Required to answer.</FormControl.Validation>}
    </FormControl>
  );
}
