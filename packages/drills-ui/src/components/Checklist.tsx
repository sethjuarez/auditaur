// Checkbox rows narrowed by a filter box, with a count of what is shown and chosen. The caller owns
// the chosen set; each checkbox carries its value as `data-check`.
import { SearchIcon } from "@primer/octicons-react";
import { Checkbox, Label, TextInput } from "@primer/react";
import { useState } from "react";
import { plural } from "../lib/format";
import "./Checklist.css";

export interface ChecklistItem {
  value: string;
  label: string;
  detail?: string;
  badge?: string;
}

interface Props {
  items: ChecklistItem[];
  chosen: ReadonlySet<string>;
  toggle: (value: string, on: boolean) => void;
  // What one item is, for the count ("store" reads "2 of 5 stores").
  noun: string;
  filterId: string;
  filterLabel: string;
  placeholder: string;
  rowsId: string;
}

const matches = (item: ChecklistItem, needle: string) =>
  [item.label, item.detail, item.value].some((text) => text?.toLowerCase().includes(needle));

export function Checklist({ items, chosen, toggle, noun, filterId, filterLabel, placeholder, rowsId }: Props) {
  const [query, setQuery] = useState("");
  const needle = query.trim().toLowerCase();
  const shown = needle ? items.filter((item) => matches(item, needle)) : items;
  const count = needle ? `Showing ${shown.length} of ${plural(items.length, noun)}` : plural(items.length, noun);
  return (
    <div className="checklist-wrap">
      <TextInput
        id={filterId}
        type="search"
        size="small"
        className="checklist-filter"
        leadingVisual={SearchIcon}
        placeholder={placeholder}
        aria-label={filterLabel}
        aria-controls={rowsId}
        value={query}
        onChange={(event) => setQuery(event.target.value)}
      />
      <div className="checklist-count muted nums" aria-live="polite">{`${count} · ${chosen.size} chosen`}</div>
      <div id={rowsId} className="checklist">
        {shown.length ? shown.map((item) => (
          <label key={item.value} className="checklist-row" title={item.value}>
            <Checkbox data-check={item.value} checked={chosen.has(item.value)} onChange={(event) => toggle(item.value, event.target.checked)} />
            <span className="checklist-label">{item.label}</span>
            {item.detail && <span className="checklist-detail path muted">{item.detail}</span>}
            {item.badge && <Label size="small" variant="secondary" className="checklist-badge">{item.badge}</Label>}
          </label>
        )) : <span className="checklist-none muted">{`No ${noun} matches “${query.trim()}”.`}</span>}
      </div>
    </div>
  );
}
