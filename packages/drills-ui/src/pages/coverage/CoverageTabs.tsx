// Which coverage list the page shows, each tab counting what it would list.
import { ChecklistIcon, CommandPaletteIcon, BroadcastIcon, TableIcon } from "@primer/octicons-react";
import { SegmentedControl } from "@primer/react";
import { shownCommands, shownEvents, shownTests, type Coverage, type CoverageTab } from "./types";

interface Props {
  coverage: Coverage;
  tab: CoverageTab;
  missedOnly: boolean;
  pick: (tab: CoverageTab) => void;
}

export function CoverageTabs({ coverage, tab, missedOnly, pick }: Props) {
  const tabs = [
    { key: "commands", label: "Commands", icon: CommandPaletteIcon, count: shownCommands(coverage, missedOnly).length, hint: "IPC commands and the tests that called them" },
    { key: "events", label: "Events", icon: BroadcastIcon, count: shownEvents(coverage, missedOnly).length, hint: "Events and the tests that emitted or received them" },
    { key: "tests", label: "Tests", icon: ChecklistIcon, count: shownTests(coverage, missedOnly).length, hint: "Tests and the commands and events each touched" },
    { key: "map", label: "Map", icon: TableIcon, count: undefined, hint: "A grid of tests against commands and events" },
  ] as const;
  return (
    <SegmentedControl size="small" aria-label="Coverage view" onChange={(index) => pick(tabs[index].key)}>
      {tabs.map((each) => (
        <SegmentedControl.Button
          key={each.key}
          data-segment={each.key}
          selected={each.key === tab}
          leadingVisual={each.icon}
          count={each.count}
          title={each.hint}
        >
          {each.label}
        </SegmentedControl.Button>
      ))}
    </SegmentedControl>
  );
}
