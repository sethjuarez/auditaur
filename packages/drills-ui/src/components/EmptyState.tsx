// What a page shows when there is nothing to list: an icon, a title, and an optional hint.
import type { Icon } from "@primer/octicons-react";
import type { ReactNode } from "react";
import "./EmptyState.css";

export function EmptyState({ icon: Glyph, title, children }: { icon: Icon; title: ReactNode; children?: ReactNode }) {
  return (
    <div className="empty-state">
      <Glyph size={24} className="empty-state-icon" />
      <div className="empty-state-title">{title}</div>
      {children && <div className="empty-state-body">{children}</div>}
    </div>
  );
}
