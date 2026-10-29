// One attachment, shown by kind: screenshots inline, DOM snapshots and other files on demand.
import { DomSnapshot } from "./DomSnapshot";
import { PayloadPanel } from "./PayloadPanel";
import { ScreenshotPanel } from "./ScreenshotPanel";
import { fileUrl, isImage, type TraceAttachment } from "./types";

interface Props {
  attachment: TraceAttachment;
  runId: string;
  // What the attachment belongs to, for image alt text, e.g. "for step 2: click #save".
  context: string;
}

const isDom = (attachment: TraceAttachment) => attachment.contentType === "application/json" && /\bdom\b/i.test(attachment.name);

export function AttachmentView({ attachment, runId, context }: Props) {
  const url = fileUrl(runId, attachment.path);
  if (isImage(attachment)) return <ScreenshotPanel attachment={attachment} url={url} context={context} />;
  if (isDom(attachment)) return <PayloadPanel attachment={attachment} url={url} render={(file) => <DomSnapshot file={file} />} />;
  return <PayloadPanel attachment={attachment} url={url} />;
}

export function AttachmentList({ attachments, runId, context }: { attachments: TraceAttachment[]; runId: string; context: string }) {
  // Screenshots first: they are what a reader looks for, and payloads stay compact below them.
  const ordered = [...attachments.filter(isImage), ...attachments.filter((each) => !isImage(each))];
  return (
    <div className="attachments">
      {ordered.map((attachment) => <AttachmentView key={`${runId}:${attachment.path}`} attachment={attachment} runId={runId} context={context} />)}
    </div>
  );
}
