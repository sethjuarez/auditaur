"""Fail when the CLI, plugin, @auditaur/api, and docs disagree about compatibility.

Offline, so it runs on every PR. The release gate (check-api-released.py) also confirms the
required api version is on npm.

- The Rust and TypeScript drive bridge protocol versions must match.
- The CLI's MIN_DRILL_API_VERSION must not exceed the api release line; requiring an api that
  isn't released leaves apps with nothing to upgrade to (AR-1).
- Docs must not tell users to install an api older than drills require.
"""

import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
RUST_PROTOCOL = ROOT / "crates/auditaur-core/src/drive_bridge.rs"
TS_PROTOCOL = ROOT / "packages/api/src/drive-bridge.ts"
MIN_API = ROOT / "crates/auditaur-cli/src/commands/drills/runner/real.rs"
MANIFEST = ROOT / ".release-please-manifest.json"
DOCS = [ROOT / "docs/src/content/docs", ROOT / "README.md", ROOT / "packages/api/README.md", ROOT / "skills"]
DOC_PIN = re.compile(r"@auditaur/api@[\^~]?(\d+\.\d+\.\d+)")


def capture(path: pathlib.Path, pattern: str) -> str:
    match = re.search(pattern, path.read_text(encoding="utf-8"), re.MULTILINE)
    if match is None:
        sys.exit(f"{path.relative_to(ROOT)}: no match for {pattern!r}; update {pathlib.Path(__file__).name}.")
    return match.group(1)


def semver(version: str) -> tuple[int, ...]:
    return tuple(int(part) for part in version.split("."))


def min_drill_api_version() -> str:
    return capture(MIN_API, r'^const MIN_DRILL_API_VERSION: &str = "(\d+\.\d+\.\d+)";')


def doc_files():
    for root in DOCS:
        if root.is_file():
            yield root
        elif root.is_dir():
            yield from (p for p in root.rglob("*") if p.suffix in {".md", ".mdx"})


def main() -> int:
    errors = []

    rust = capture(RUST_PROTOCOL, r"^pub const DRIVE_BRIDGE_PROTOCOL_VERSION: \w+ = (\d+);")
    ts = capture(TS_PROTOCOL, r"^const PROTOCOL_VERSION = (\d+);")
    if rust != ts:
        errors.append(f"Drive bridge protocol mismatch: Rust {rust}, TypeScript {ts}.")

    required = min_drill_api_version()
    released = json.loads(MANIFEST.read_text(encoding="utf-8"))["packages/api"]
    if semver(required) > semver(released):
        errors.append(
            f"MIN_DRILL_API_VERSION {required} is newer than the @auditaur/api release line {released}. "
            "Release the api first ('Release Frontend API' workflow), then raise the CLI minimum."
        )

    for doc in doc_files():
        for line_no, line in enumerate(doc.read_text(encoding="utf-8").splitlines(), 1):
            for pin in DOC_PIN.findall(line):
                if semver(pin) < semver(required):
                    errors.append(
                        f"{doc.relative_to(ROOT)}:{line_no}: @auditaur/api@{pin} is older than "
                        f"MIN_DRILL_API_VERSION {required}."
                    )

    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"Compatible: protocol {rust}; drills need @auditaur/api >= {required} (released {released}).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
