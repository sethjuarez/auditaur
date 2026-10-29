"""Fail when @auditaur/api has shipped-code changes that no published api release covers.

The CLI and plugin release through release-plz; @auditaur/api releases separately through
release-please. A Rust release that depends on unreleased frontend bridge changes leaves apps
with nothing to upgrade to, so the Rust release gates on this check.
"""

import json
import pathlib
import shutil
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
PACKAGE = "@auditaur/api"
# Matches the build boundary in packages/api/tsconfig.build.json: tests don't ship.
SHIPPED = ["packages/api/src", ":(exclude,glob)packages/api/src/**/*.test.ts"]
FIX = (
    "Run the 'Release Frontend API' workflow, merge its release PR, and run it again "
    "to publish before releasing the Rust crates."
)


def unreleased_commits(tag: str) -> str | None:
    try:
        return subprocess.check_output(
            ["git", "log", "--format=%h %s", f"{tag}..HEAD", "--", *SHIPPED],
            cwd=ROOT,
            text=True,
        ).strip()
    except subprocess.CalledProcessError:
        return None


def published(version: str, attempts: int = 6, delay: float = 15) -> bool:
    npm = shutil.which("npm")
    if npm is None:
        sys.exit("npm is required to check the published @auditaur/api version.")
    for attempt in range(attempts):
        result = subprocess.run(
            [npm, "view", f"{PACKAGE}@{version}", "version", "--prefer-online"],
            capture_output=True,
            text=True,
        )
        if result.returncode == 0 and result.stdout.strip() == version:
            return True
        if attempt + 1 < attempts:
            time.sleep(delay)
    return False


def main() -> int:
    manifest = json.loads((ROOT / ".release-please-manifest.json").read_text(encoding="utf-8"))
    version = manifest["packages/api"]
    tag = f"api-v{version}"

    log = unreleased_commits(tag)
    if log is None:
        print(f"Cannot read history since {tag}; fetch tags (fetch-depth: 0).", file=sys.stderr)
        return 1
    if log:
        print(f"packages/api/src changed since {tag} without an api release:", file=sys.stderr)
        print(log, file=sys.stderr)
        print(FIX, file=sys.stderr)
        return 1

    if not published(version):
        print(f"{PACKAGE}@{version} is tagged but not on npm.", file=sys.stderr)
        print("Check the 'Publish Frontend API' run for that tag.", file=sys.stderr)
        return 1

    print(f"{PACKAGE}@{version} is published and covers packages/api/src.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
