// A text field the server owns: typing is sent after a short pause, and the server's value replaces
// the field unless the user is mid-edit (a change not yet sent).
import { useEffect, useRef, useState } from "react";

export function useDebouncedField(serverValue: string, send: (value: string) => void, delayMs = 150) {
  const [draft, setDraft] = useState(serverValue);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (timer.current === null) setDraft(serverValue);
  }, [serverValue]);
  useEffect(() => () => clearTimeout(timer.current ?? undefined), []);

  const onChange = (value: string) => {
    setDraft(value);
    clearTimeout(timer.current ?? undefined);
    timer.current = setTimeout(() => {
      timer.current = null;
      send(value);
    }, delayMs);
  };
  return [draft, onChange] as const;
}
