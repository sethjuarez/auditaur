// A text attachment (a DOM snapshot, ARIA YAML, JSON…) behind a disclosure, loaded the first time
// it opens. `render` shows the loaded file; by default it is the raw (pretty-printed) text.
import { FileIcon } from "@primer/octicons-react";
import { useState, type ReactNode } from "react";
import { useFileText, type FileText } from "../../hooks/useFileText";
import { RawText } from "./RawText";
import type { TraceAttachment } from "./types";

interface Props {
  attachment: TraceAttachment;
  url: string;
  label?: string;
  render?: (file: FileText) => ReactNode;
}

export function PayloadPanel({ attachment, url, label, render }: Props) {
  const [open, setOpen] = useState(false);
  const file = useFileText(url, open);
  let body: ReactNode;
  if (file.error) body = <pre className="payload-pre mono payload-error">{`Could not load: ${file.error}`}</pre>;
  else if (file.text == null) body = <pre className="payload-pre mono muted">Loading…</pre>;
  else body = render ? render(file) : <RawText text={file.text} />;
  return (
    <details className="payload" data-path={attachment.path} onToggle={(event) => setOpen(event.currentTarget.open)}>
      <summary>
        <FileIcon size={16} className="payload-icon" aria-hidden="true" />
        <span className="payload-name">{label ?? attachment.name}</span>
        <span className="path muted payload-path">{attachment.path}</span>
      </summary>
      <div className="payload-body">{body}</div>
    </details>
  );
}
