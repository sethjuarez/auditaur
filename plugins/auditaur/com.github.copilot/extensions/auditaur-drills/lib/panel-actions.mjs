// Agent actions about the panel itself: capture it, report its build health, and cancel its run.

import { captureScreenshot } from "./browser.mjs";
import { health } from "./build-info.mjs";
import { currentView } from "./current-view.mjs";
import { cancelRun } from "./launcher.mjs";
import { runIdProperty } from "./schemas.mjs";

export function panelActions() {
    return [
        {
            name: "screenshot",
            description: "Capture the panel as the user sees it (PNG), with its current view.",
            readOnly: true,
            inputSchema: {
                type: "object",
                additionalProperties: false,
                properties: {
                    width: { type: "integer", minimum: 320, maximum: 3840, description: "Viewport width in pixels (default 1280)." },
                    height: { type: "integer", minimum: 200, maximum: 4000, description: "Viewport height in pixels (default 800)." },
                },
            },
            handler: async (panel, input, host) => {
                const png = await captureScreenshot(host.url, input);
                return {
                    textResultForLlm: JSON.stringify(await currentView(panel)),
                    binaryResultsForLlm: [{ type: "image", mimeType: "image/png", data: png.toString("base64"), description: "Auditaur drills panel" }],
                    resultType: "success",
                };
            },
        },
        {
            name: "health",
            description: "Which build the canvas runs, whether its files changed on disk since (then extensions need a reload), its install and plugin version, the CLI version and any mismatch, and the panel's route.",
            readOnly: true,
            handler: async (panel) => ({ ...(await health(panel.shared.cli)), route: panel.route }),
        },
        {
            name: "cancel",
            description: "Stop a drill run in progress; it finishes as interrupted. Defaults to the run this panel launched, then the newest running one.",
            inputSchema: { type: "object", additionalProperties: false, properties: { runId: runIdProperty } },
            handler: (panel, input) => cancelRun(panel.shared, input.runId ?? null),
        },
    ];
}