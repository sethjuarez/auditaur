// `screenshot` for every canvas: the agent sees the panel as the user does, plus the view it shows.
// The result is a tool result object, so the image reaches the model rather than a JSON blob.

import { captureScreenshot } from "./browser.mjs";
import { currentView } from "./view-actions.mjs";

export function screenshotAction(canvas) {
    return {
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
        handler: async (state, input, panel) => {
            const png = await captureScreenshot(panel.url, input);
            const view = await currentView(canvas, state);
            return {
                textResultForLlm: JSON.stringify(view),
                binaryResultsForLlm: [
                    { type: "image", mimeType: "image/png", data: png.toString("base64"), description: `${canvas.displayName} panel` },
                ],
                resultType: "success",
            };
        },
    };
}
