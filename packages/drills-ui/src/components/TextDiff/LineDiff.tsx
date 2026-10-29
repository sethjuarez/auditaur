// A line diff as the runner writes it: `- ` lines were expected, `+ ` lines are what it got.
import "./TextDiff.css";

const kindOf = (line: string) => (line.startsWith("- ") ? "del" : line.startsWith("+ ") ? "ins" : null);

export function LineDiff({ text, label }: { text: string; label: string }) {
  const lines = text.split("\n");
  const removed = lines.filter((line) => kindOf(line) === "del").length;
  const added = lines.filter((line) => kindOf(line) === "ins").length;
  return (
    <figure className="line-diff">
      <figcaption className="line-diff-legend">
        <span className="visually-hidden">{`${label}: `}</span>
        <span className="line-diff-count del">{`− ${removed} expected`}</span>
        <span className="line-diff-count ins">{`+ ${added} actual`}</span>
      </figcaption>
      <pre className="compare-lines mono">
        {lines.map((line, index) => {
          const kind = kindOf(line);
          return <span key={index} className={`line${kind ? ` ${kind}` : ""}`}>{line || " "}</span>;
        })}
      </pre>
    </figure>
  );
}
