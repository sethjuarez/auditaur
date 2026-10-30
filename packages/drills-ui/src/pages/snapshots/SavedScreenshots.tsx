// The run's named `screenshot` steps: each image with the test and step that saved it.
import { fileUrl } from "../../lib/files";
import { plural } from "../../lib/format";
import type { SavedScreenshot } from "./types";

export function SavedScreenshots({ shots, runId }: { shots: SavedScreenshot[]; runId: string }) {
  if (!shots.length) return null;
  return (
    <section className="snap-shots" aria-label="Saved screenshots">
      <h2 className="snap-shots-title">{plural(shots.length, "saved screenshot")}</h2>
      <div className="snap-shots-grid">
        {shots.map((shot) => {
          const url = fileUrl(runId, shot.file.path);
          return (
            <figure key={shot.id} className="box snap-shot">
              <a href={url} target="_blank" rel="noreferrer">
                <img src={url} alt={shot.step} loading="lazy" />
              </a>
              <figcaption>
                <span className="snap-test muted">{shot.test}</span>
                <span className="snap-shot-step">{shot.step}</span>
              </figcaption>
            </figure>
          );
        })}
      </div>
    </section>
  );
}
