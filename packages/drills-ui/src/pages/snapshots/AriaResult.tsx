// An ARIA snapshot result: the runner's line diff, then the full expected and actual files.
import { LineDiff, TextFileDisclosure } from "../../components/TextDiff";
import { fileUrl } from "../../lib/files";
import type { SnapshotItem } from "./types";

const FILES = [
  { name: "expected", title: "Expected (baseline)" },
  { name: "actual", title: "Actual (this run)" },
] as const;

export function AriaResult({ item, runId }: { item: SnapshotItem; runId: string }) {
  return (
    <>
      {item.message && <LineDiff text={item.message} label="Differences from the baseline" />}
      {FILES.map(({ name, title }) => {
        const file = item.files[name];
        return file ? <TextFileDisclosure key={name} url={fileUrl(runId, file.path)} title={title} path={file.path} /> : null;
      })}
    </>
  );
}
