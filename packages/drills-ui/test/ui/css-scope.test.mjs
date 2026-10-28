// All pages share one stylesheet, so a class one page styles at the top level must not be styled by another page.
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const pages = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "src", "pages");

function cssFiles(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    return entry.isDirectory() ? cssFiles(path) : path.endsWith(".css") ? [path] : [];
  });
}

// The first class of each selector: the element a rule anchors on.
function anchors(css) {
  const found = new Set();
  for (const [, prelude] of css.replace(/\/\*[\s\S]*?\*\//g, "").matchAll(/([^{}]+)\{/g)) {
    for (const selector of prelude.split(",")) {
      const first = selector.trim().match(/^\.([A-Za-z][\w-]*)/);
      if (first) found.add(first[1]);
    }
  }
  return found;
}

test("pages do not style the same class", () => {
  const owners = new Map();
  for (const file of cssFiles(pages)) {
    const page = relative(pages, file).split(/[\\/]/)[0];
    for (const name of anchors(readFileSync(file, "utf8"))) {
      owners.set(name, new Set([...(owners.get(name) ?? []), page]));
    }
  }
  const shared = [...owners].filter(([, owning]) => owning.size > 1).map(([name, owning]) => `.${name}: ${[...owning].join(", ")}`);
  assert.deepEqual(shared, [], "Prefix page classes with the page name, or move shared rules to components/ or theme/.");
});
