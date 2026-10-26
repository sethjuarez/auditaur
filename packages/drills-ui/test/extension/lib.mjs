// The extension's modules, by path from this package.
export const LIB = new URL("../../../../.github/extensions/auditaur-drills/lib/", import.meta.url);
export const lib = (name) => import(new URL(name, LIB));

export const RUN = "20250101-120000-000";