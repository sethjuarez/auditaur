// Builds the drill canvas pages into one bundle the extension serves: ui/app.js and ui/app.css.
// Primer's theme files define ~2,000 variables each; only the ones the bundle uses (plus whatever
// those refer to) are kept.

import { build } from "esbuild";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { bridgeCss } from "./theme-bridge.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, "..", "..", ".github", "extensions", "auditaur-drills", "ui");
const mirror = join(here, "..", "..", "crates", "auditaur-cli", "assets", "auditaur-drills", "ui");
const primitives = dirname(createRequire(import.meta.url).resolve("@primer/primitives/package.json"));
const css = (path) => readFile(join(primitives, "dist", "css", path), "utf8");

// Size and type scales are small, so they ship whole; the light and dark themes are trimmed.
const SCALES = ["base/size/size.css", "base/typography/typography.css", "functional/size/border.css", "functional/size/radius.css", "functional/size/size.css", "functional/typography/typography.css"];
const THEMES = ["functional/themes/light.css", "functional/themes/dark.css"];

const options = {
    absWorkingDir: here,
    entryPoints: [join(here, "src", "main.tsx")],
    bundle: true,
    minify: true,
    format: "esm",
    target: "es2022",
    jsx: "automatic",
    outdir: out,
    entryNames: "app",
    write: false,
    logLevel: "warning",
    define: { "process.env.NODE_ENV": '"production"' },
};

// esbuild keeps a module's CSS import even when tree-shaking drops the module, which would ship
// the styles of every Primer component. So: find the modules that survive, then build again with
// the CSS of all the others left out.
const probe = await build({ ...options, metafile: true, loader: { ".css": "empty" } });
const kept = new Set(Object.values(probe.metafile.outputs).flatMap((output) =>
    Object.entries(output.inputs).filter(([, input]) => input.bytesInOutput > 0).map(([path]) => resolve(here, path))));
const unusedStyles = {
    name: "unused-styles",
    setup(builder) {
        builder.onResolve({ filter: /\.css$/ }, (args) =>
            args.importer && !args.importer.startsWith(join(here, "src")) && !kept.has(resolve(args.importer)) ? { path: args.path, namespace: "unused-style" } : undefined);
        builder.onLoad({ filter: /.*/, namespace: "unused-style" }, () => ({ contents: "", loader: "css" }));
    },
};
const result = await build({ ...options, plugins: [unusedStyles] });
const outputs = Object.fromEntries(result.outputFiles.map((file) => [file.path.slice(file.path.lastIndexOf(".") + 1), file.text]));

const DECLARATION = /(--[\w-]+)\s*:\s*([^;{}]*);(\s*\/\*\*.*?\*\/)?/g;
const referenced = (text) => [...text.matchAll(/var\(\s*(--[\w-]+)/g)].map((match) => match[1]);

// Every variable the bundle reads, then (repeatedly) every variable those definitions read.
function usedVariables(sources, themes) {
    const definitions = new Map();
    for (const theme of themes) for (const [, name, value] of theme.matchAll(DECLARATION)) definitions.set(name, [...(definitions.get(name) ?? []), value]);
    const used = new Set(sources.flatMap(referenced));
    const queue = [...used];
    while (queue.length) {
        for (const value of definitions.get(queue.pop()) ?? []) {
            for (const name of referenced(value)) if (!used.has(name)) used.add(name), queue.push(name);
        }
    }
    return used;
}

const scales = await Promise.all(SCALES.map(css));
const themes = await Promise.all(THEMES.map(css));
const defined = new Set([...scales, ...themes].flatMap((text) => [...text.matchAll(DECLARATION)].map((match) => match[1])));
const usedByBundle = usedVariables([outputs.js, outputs.css, ...scales], themes);
const bridge = bridgeCss(usedByBundle, defined);
if (bridge.missing.length) throw new Error(`theme-bridge.mjs maps roles Primer doesn't define: ${bridge.missing.join(", ")}`);
// Each bridged role falls back to `--<role>-primer`: Primer's own value, emitted beside it.
const aliased = new Set(bridge.roles);
const withAliases = (text) => text.replace(DECLARATION, (all, name, value) => (aliased.has(name) ? `${all}${name}-primer:${value.trim()};` : all));
const used = usedVariables([outputs.js, outputs.css, ...scales, bridge.css], themes);
const trimmed = themes.map((theme) => withAliases(theme.replace(/\/\*\*[\s\S]*?\*\/\s*/, "").replace(DECLARATION, (all, name, value) => (used.has(name) ? `${name}:${value.trim()};` : "")).replace(/\n\s*\n/g, "\n")));
const app = { js: outputs.js, css: [...scales.map(withAliases), ...trimmed, outputs.css, bridge.css].join("\n") };

// The extension serves ui/ from .github/extensions; `auditaur init` embeds the crate's copy.
for (const dir of [out, mirror]) {
    await mkdir(dir, { recursive: true });
    await writeFile(join(dir, "app.js"), app.js);
    await writeFile(join(dir, "app.css"), app.css);
}
const kb = (text) => `${Math.round(Buffer.byteLength(text) / 1024)} KB`;
process.stdout.write(`ui/app.js ${kb(app.js)}, ui/app.css ${kb(app.css)}, ${bridge.roles.length} roles bridged to the host theme\n`);
