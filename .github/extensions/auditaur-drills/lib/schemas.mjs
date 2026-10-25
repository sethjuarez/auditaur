// Input schema pieces shared by more than one canvas.
import { RUN_ID } from "./paths.mjs";

export const cwdProperty = {
    type: "string",
    description: "Folder containing .auditaur/drills, relative to the repository root. Defaults to the root.",
};

export const runIdProperty = {
    type: "string",
    pattern: RUN_ID.source,
    description: "Show this run instead of following the newest one.",
};
