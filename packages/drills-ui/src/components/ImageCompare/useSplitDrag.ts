// Pointer dragging for a split slider: the primary button only, captured so the drag keeps going
// outside the element, live values while moving, and one commit when it ends or is cancelled.
import { useEffect, useRef, type PointerEvent } from "react";

const clamp = (value: number) => Math.min(100, Math.max(0, value));

const percentAt = (event: PointerEvent<HTMLElement>) => {
  const rect = event.currentTarget.getBoundingClientRect();
  return rect.width ? clamp(Math.round(((event.clientX - rect.left) / rect.width) * 100)) : 50;
};

export function useSplitDrag(onLive: (value: number) => void, onCommit: (value: number) => void, onStart: () => void, onCancel: () => void) {
  const drag = useRef<{ pointerId: number; value: number } | null>(null);
  const cancel = useRef(onCancel);
  cancel.current = onCancel;
  // Unmounted mid-drag (mode switch, reload): the release events never come, so drop the live value.
  useEffect(
    () => () => {
      if (drag.current) {
        drag.current = null;
        cancel.current();
      }
    },
    [],
  );
  const end = (event: PointerEvent<HTMLElement>) => {
    if (drag.current?.pointerId !== event.pointerId) return;
    const { value } = drag.current;
    drag.current = null;
    onCommit(value);
  };
  return {
    onPointerDown: (event: PointerEvent<HTMLElement>) => {
      if (event.button !== 0 || !event.isPrimary) return;
      event.preventDefault();
      // Synthetic or already-released pointers can't be captured; the drag still works inside.
      try {
        event.currentTarget.setPointerCapture(event.pointerId);
      } catch {}
      onStart();
      const value = percentAt(event);
      drag.current = { pointerId: event.pointerId, value };
      onLive(value);
    },
    onPointerMove: (event: PointerEvent<HTMLElement>) => {
      if (drag.current?.pointerId !== event.pointerId) return;
      const value = percentAt(event);
      if (value === drag.current.value) return;
      drag.current.value = value;
      onLive(value);
    },
    onPointerUp: end,
    onPointerCancel: end,
    onLostPointerCapture: end,
  };
}

// The split a key moves to: ±1 per arrow, ±10 with Shift or Page Up/Down, Home and End to the ends.
export function splitForKey(key: string, shift: boolean, value: number): number | null {
  const step = shift ? 10 : 1;
  switch (key) {
    case "ArrowLeft":
    case "ArrowDown":
      return clamp(value - step);
    case "ArrowRight":
    case "ArrowUp":
      return clamp(value + step);
    case "PageDown":
      return clamp(value - 10);
    case "PageUp":
      return clamp(value + 10);
    case "Home":
      return 0;
    case "End":
      return 100;
    default:
      return null;
  }
}
