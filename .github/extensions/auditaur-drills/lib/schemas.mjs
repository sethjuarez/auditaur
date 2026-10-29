// Input schema pieces shared by more than one module.
import { RUN_ID } from "./paths.mjs";

export const cwdProperty = {
    type: "string",
    description: "Folder containing .auditaur/drills, relative to the repository root. Defaults to the root.",
};

export const runIdProperty = {
    type: "string",
    pattern: RUN_ID.source,
    description: "A drill run id; omit to use the newest run.",
};
