// One captioned image of a comparison.
export function CompareFigure({ url, caption, alt }: { url: string; caption: string; alt: string }) {
  return (
    <figure className="compare-figure">
      <img alt={alt} src={url} />
      <figcaption>{caption}</figcaption>
    </figure>
  );
}
