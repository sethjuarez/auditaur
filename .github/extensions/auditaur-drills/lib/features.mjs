// Loads each page's feature module on its own, so one that fails to import or doesn't have the
// feature shape takes down only its page: the others keep working and `health` names what broke.

const PAGE_MODULES = ["tests", "run", "trace", "snapshots", "coverage"];

// A feature: `{ page, view, routes(state), actions[], describe?(state), sync?(state) }`.
function checkFeature(feature, page) {
    if (feature?.page !== page) throw new Error(`exports page \`${feature?.page}\`, expected \`${page}\``);
    if (typeof feature.routes !== "function") throw new Error("has no routes(state)");
    if (feature.actions != null && !Array.isArray(feature.actions)) throw new Error("actions is not a list");
    for (const action of feature.actions ?? []) {
        if (typeof action?.name !== "string" || typeof action.handler !== "function") throw new Error("has an action without a name and handler");
    }
    return feature;
}

// `{ features, failed: [{ page, error }] }`.
export async function loadFeatures(pages = PAGE_MODULES) {
    const results = await Promise.all(pages.map(async (page) => {
        try {
            const module = await import(`../features/${page}/index.mjs`);
            return { feature: checkFeature(module.default, page) };
        } catch (error) {
            return { failed: { page, error: String(error?.stack ?? error).split("\n").slice(0, 3).join("\n") } };
        }
    }));
    return {
        features: results.flatMap((result) => (result.feature ? [result.feature] : [])),
        failed: results.flatMap((result) => (result.failed ? [result.failed] : [])),
    };
}
