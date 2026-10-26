// The gate page's `/state`, as features/gate/view.mjs builds it.
import type { Gate } from "../../components/gate";

// none: no gate given. answered: the drill has not picked the answer up yet. closed: the request is
// gone (answered, timed out, or the drill stopped).
export type GateStatus = "none" | "pending" | "answered" | "expired" | "closed";

export interface GateSessionInfo {
  sessionId?: string;
  instanceId?: string;
  pid?: number;
  databasePath?: string;
}

export interface GateState {
  status: GateStatus;
  requestId: string | null;
  requestPath: string | null;
  gate: Gate | null;
  session: GateSessionInfo | null;
  expiresAt?: string | null;
  // The page has no view settings.
  ui: Record<string, never>;
}

// Poll fast while the answer could still change what shows.
export const pollMsFor = (state: GateState | null) =>
  state?.status === "pending" || state?.status === "answered" ? 1000 : 5000;
