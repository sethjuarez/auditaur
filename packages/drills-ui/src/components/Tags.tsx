// Drill tags as small labels, written the way drills spell them (`@smoke`).
import { Label } from "@primer/react";

export function Tags({ tags }: { tags: string[] }) {
  return <>{tags.map((tag) => <Label key={tag} size="small" variant="secondary" className="tag">@{tag}</Label>)}</>;
}
