// A list row that opens to show detail: chevron, status, and name in a disclosure button, then any
// trailing content (history, duration, row actions), then the detail when open. The whole row
// toggles on click; keyboard and screen readers use the disclosure button, whose native activation
// click bubbles to the row. Trailing buttons stop their clicks so they never toggle the row.
import { ChevronDownIcon, ChevronRightIcon } from "@primer/octicons-react";
import { useId, type MouseEvent, type ReactNode } from "react";
import "./Box.css";
import "./DisclosureRow.css";

interface Props {
  id: string;
  open: boolean;
  toggle: () => void;
  status: ReactNode;
  name: ReactNode;
  trailing?: ReactNode;
  children?: ReactNode;
  className?: string;
}

export function DisclosureRow({ id, open, toggle, status, name, trailing, children, className }: Props) {
  const detailId = useId();
  // Primer renders a trailing button's tooltip inside the row; clicking it must not toggle.
  const onRowClick = (event: MouseEvent) => {
    if (!(event.target as Element).closest('[role="tooltip"]')) toggle();
  };
  const Chevron = open ? ChevronDownIcon : ChevronRightIcon;
  return (
    <div className={`box-row test-item${open ? " open" : ""}${className ? ` ${className}` : ""}`}>
      <div className="test clickable" data-id={id} onClick={onRowClick}>
        <button type="button" className="test-toggle" aria-expanded={open} aria-controls={open ? detailId : undefined}>
          <Chevron size={16} className="chevron" />
          {status}
          <span className="test-name">{name}</span>
        </button>
        {trailing}
      </div>
      {open && <div className="detail" id={detailId}>{children}</div>}
    </div>
  );
}