// Which corner labels of a split slider the divider has crossed, so they can fade instead of
// sitting over the other image.
import { useLayoutEffect, useRef, useState, type RefObject } from "react";

export function useCoveredLabels(root: RefObject<HTMLElement | null>, split: number) {
  const left = useRef<HTMLSpanElement>(null);
  const right = useRef<HTMLSpanElement>(null);
  const [covered, setCovered] = useState({ left: false, right: false });
  useLayoutEffect(() => {
    const box = root.current?.getBoundingClientRect();
    const leftBox = left.current?.getBoundingClientRect();
    const rightBox = right.current?.getBoundingClientRect();
    if (!box?.width || !leftBox || !rightBox) return;
    const divider = box.left + (box.width * split) / 100;
    const next = { left: divider < leftBox.right, right: divider > rightBox.left };
    setCovered((prev) => (prev.left === next.left && prev.right === next.right ? prev : next));
  });
  return { left, right, covered };
}
