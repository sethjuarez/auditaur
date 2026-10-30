// A refusal the user can act on. The entry point turns it into a CanvasError for the agent, and
// the panel server into a 409, so domain modules never depend on the SDK.
export class DrillError extends Error {
    constructor(code, message) {
        super(message);
        this.name = "DrillError";
        this.code = code;
    }
}

// What an agent sees. Some hosts show only the message, so the message carries the code too.
export const agentMessage = (error) => (error.message.startsWith(`${error.code}:`) ? error.message : `${error.code}: ${error.message}`);
