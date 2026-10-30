// A human gate as the server describes it (lib/gates.mjs for a run's gates, features/gate/view.mjs
// for a lone `auditaur drill` gate), and the answer the form sends.

export interface GateChoice { id: string; label: string; outcome: string }
export interface GateInput { id: string; label?: string; kind?: string; required?: boolean; sensitive?: boolean }

export interface Gate {
  requestId: string | null;
  test: string;
  attempt?: number;
  since: string | null;
  // When the runner stops waiting (its own deadline); the countdown prefers it to since + timeoutMs.
  expiresAt?: string | null;
  name: string;
  instructions: string;
  selector: string | null;
  expectText: string | null;
  timeoutMs: number | null;
  manualContinue: boolean;
  choices: GateChoice[];
  inputs: GateInput[];
  clipboard: { label: string; value?: string; sensitive?: boolean; redacted?: boolean; status: string } | null;
}

export interface GateAnswer { requestId: string; choiceId: string | null; inputs: { id: string; value: string }[] }
