// A screenshot attachment: the image at full panel width, captioned with its name and file.
import type { TraceAttachment } from "./types";

export function ScreenshotPanel({ attachment, url, context }: { attachment: TraceAttachment; url: string; context: string }) {
  return (
    <figure className="shot-figure">
      <img className="shot" loading="lazy" alt={`${attachment.name} ${context}`} src={url} />
      <figcaption className="muted">
        {attachment.name} · <span className="path">{attachment.path}</span>
      </figcaption>
    </figure>
  );
}
