// A text file behind a disclosure, fetched the first time it opens (by click or by script).
import { FileIcon } from "@primer/octicons-react";
import { useState, type ReactNode } from "react";
import { useFileText } from "../../hooks/useFileText";
import "./TextDiff.css";

interface Props {
  url: string;
  title: ReactNode;
  path?: string;
}

export function TextFileDisclosure({ url, title, path }: Props) {
  const [open, setOpen] = useState(false);
  const file = useFileText(url, open);
  return (
    <details className="compare-text" onToggle={(event) => setOpen(event.currentTarget.open)}>
      <summary>
        <FileIcon size={16} className="compare-text-icon" />
        <span className="compare-text-title">{title}</span>
        {path && <span className="compare-text-path path muted">{path}</span>}
      </summary>
      <pre className={`compare-text-body mono${file.error ? " error" : ""}`}>
        {file.error ? `Could not load: ${file.error}` : (file.text ?? "Loading…")}
      </pre>
    </details>
  );
}
