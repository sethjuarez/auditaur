// The sticky page header: title, an optional "open in" group and meta, the page's actions on the
// right, then any toolbar rows below.
import type { ReactNode } from "react";
import "./PageHeader.css";

interface Props {
  title: string;
  meta?: ReactNode;
  actions?: ReactNode;
  children?: ReactNode;
}

export function PageHeader({ title, meta, actions, children }: Props) {
  return (
    <header className="page-header">
      <div className="page-header-top">
        <h1 className="page-title">{title}</h1>
        {meta}
        <span className="spacer" />
        {actions && <div className="page-actions">{actions}</div>}
      </div>
      {children}
    </header>
  );
}
