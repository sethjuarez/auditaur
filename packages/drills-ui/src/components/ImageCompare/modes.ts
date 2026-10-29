import { ArrowBothIcon, ColumnsIcon, DiffIcon, type Icon } from "@primer/octicons-react";

export type CompareMode = "side" | "slider" | "diff";

export const COMPARE_MODES: { mode: CompareMode; label: string; icon: Icon; hint: string }[] = [
  { mode: "side", label: "Side by side", icon: ColumnsIcon, hint: "Show expected and actual next to each other" },
  { mode: "slider", label: "Slider", icon: ArrowBothIcon, hint: "Overlay expected on actual and drag to compare" },
  { mode: "diff", label: "Diff", icon: DiffIcon, hint: "Show the pixel diff image" },
];
