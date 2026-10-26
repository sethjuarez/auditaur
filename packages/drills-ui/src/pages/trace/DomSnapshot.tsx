// A DOM snapshot attachment: the page's title and URL, then its visible text and HTML. Falls back
// to the raw file when it isn't the shape the drill runner writes.
import type { FileText } from "../../hooks/useFileText";
import { RawText } from "./RawText";

interface Captured { value?: unknown; truncated?: unknown; length?: unknown }
interface Snapshot { title?: unknown; url?: unknown; bodyText?: Captured; html?: Captured }

const textOf = (captured: Captured | undefined) => (typeof captured?.value === "string" ? captured.value : null);

function Part({ heading, captured }: { heading: string; captured: Captured | undefined }) {
  const text = textOf(captured);
  if (text == null) return null;
  return (
    <section className="dom-part">
      <h4 className="dom-heading">
        {heading}
        {captured?.truncated === true && <span className="muted"> (truncated)</span>}
      </h4>
      <RawText text={text} />
    </section>
  );
}

export function DomSnapshot({ file }: { file: FileText }) {
  const snapshot = file.json as Snapshot | undefined;
  if (!snapshot || typeof snapshot !== "object" || (textOf(snapshot.bodyText) == null && textOf(snapshot.html) == null)) {
    return <RawText text={file.text ?? ""} />;
  }
  return (
    <div className="dom-snapshot">
      {(typeof snapshot.title === "string" || typeof snapshot.url === "string") && (
        <div className="dom-meta muted">
          {typeof snapshot.title === "string" && <span>{snapshot.title}</span>}
          {typeof snapshot.url === "string" && <span className="path">{snapshot.url}</span>}
        </div>
      )}
      <Part heading="Page text" captured={snapshot.bodyText} />
      <Part heading="HTML" captured={snapshot.html} />
    </div>
  );
}
