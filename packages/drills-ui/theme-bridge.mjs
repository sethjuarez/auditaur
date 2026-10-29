// Primer role -> Copilot app canvas token. Inside the app (it sets `data-theme-source` on the root),
// these make Primer components take the app's own colors, so the canvas matches the app exactly;
// standalone, Primer's themes apply as-is. Host names come from the app's canvas theme contract
// (github/github-app src/lib/extensionCanvasThemeContract.json); each role falls back to Primer's
// own value, so a host that omits a token never leaves the role invalid.

const STATES = ["rest", "hover", "active", "disabled"];
const TONES = ["accent", "attention", "danger", "success", "done", "severe"];

const pairs = [
    ["--bgColor-default", "--background-color-default"],
    ["--bgColor-muted", "--background-color-muted"],
    ["--bgColor-emphasis", "--background-color-emphasis"],
    ["--bgColor-disabled", "--background-color-disabled"],
    ["--bgColor-neutral-emphasis", "--background-color-neutral-emphasis"],
    ["--bgColor-neutral-muted", "--background-color-neutral-muted"],
    ...TONES.flatMap((tone) => [
        [`--bgColor-${tone}-emphasis`, `--background-color-${tone}-emphasis`],
        [`--bgColor-${tone}-muted`, `--background-color-${tone}-muted`],
        [`--fgColor-${tone}`, `--text-color-${tone}`],
    ]),
    ["--fgColor-default", "--text-color-default"],
    ["--fgColor-muted", "--text-color-muted"],
    ["--fgColor-link", "--text-color-link"],
    ["--fgColor-disabled", "--text-color-disabled"],
    ["--fgColor-neutral", "--text-color-neutral"],
    ["--fgColor-onEmphasis", "--text-color-on-emphasis"],
    ["--borderColor-default", "--border-color-default"],
    ["--borderColor-muted", "--border-color-muted"],
    ["--borderColor-emphasis", "--border-color-emphasis"],
    ...["accent", "attention", "danger", "success"].flatMap((tone) => [
        [`--borderColor-${tone}-emphasis`, `--border-color-${tone}-emphasis`],
        [`--borderColor-${tone}-muted`, `--border-color-${tone}-muted`],
    ]),
    ["--focus-outlineColor", "--color-focus-outline"],
    ["--focus-outline-color", "--color-focus-outline"],
    ["--overlay-bgColor", "--background-color-overlay"],
    ["--overlay-backdrop-bgColor", "--background-color-overlay-backdrop"],
    ...["default", "primary", "danger", "invisible"].flatMap((variant) => STATES.flatMap((state) => [
        [`--button-${variant}-bgColor-${state}`, `--background-color-button-${variant}-${state}`],
        [`--button-${variant}-borderColor-${state}`, `--border-color-button-${variant}-${state}`],
        [`--button-${variant}-fgColor-${state}`, `--text-color-button-${variant}-${state}`],
    ])),
    ...STATES.map((state) => [`--control-bgColor-${state}`, `--background-color-control-${state}`]),
    ["--control-bgColor-selected", "--background-color-control-selected"],
    ...[...STATES, "selected"].map((state) => [`--control-transparent-bgColor-${state}`, `--background-color-control-transparent-${state}`]),
    ...STATES.map((state) => [`--control-checked-bgColor-${state}`, `--background-color-control-checked-${state}`]),
    ["--control-borderColor-rest", "--border-color-control-rest"],
    ["--control-borderColor-emphasis", "--border-color-control-emphasis"],
    ["--control-borderColor-disabled", "--border-color-control-disabled"],
    ["--control-borderColor-selected", "--border-color-control-selected"],
    ["--control-fgColor-rest", "--text-color-control-rest"],
    ["--control-fgColor-placeholder", "--text-color-control-placeholder"],
    ["--control-fgColor-disabled", "--text-color-control-disabled"],
    ["--controlTrack-bgColor-rest", "--background-color-segmentedControl-bg-rest"],
    ["--controlKnob-bgColor-rest", "--background-color-segmentedControl-button-bg-rest"],
    ["--fontStack-sansSerif", "--font-sans"],
    ["--fontStack-monospace", "--font-mono"],
];

// Button roles that Primer or the host contract doesn't define.
const UNPAIRED = new Set([
    "--button-default-fgColor-hover", "--button-default-fgColor-active",
    "--button-primary-fgColor-hover", "--button-primary-fgColor-active",
    "--button-danger-borderColor-disabled",
    "--button-invisible-borderColor-active", "--button-invisible-borderColor-disabled",
]);

export const BRIDGE = new Map(pairs.filter(([role]) => !UNPAIRED.has(role)));

/** The bridge rule for the roles the bundle actually uses; `missing` lists used roles Primer lacks. */
export function bridgeCss(used, defined) {
    const roles = [...BRIDGE.keys()].filter((role) => used.has(role));
    const missing = roles.filter((role) => !defined.has(role));
    const body = roles.map((role) => `  ${role}: var(${BRIDGE.get(role)}, var(${role}-primer));`).join("\n");
    return { roles, missing, css: `html[data-theme-source] .drills-app {\n${body}\n}\n` };
}
