"""Fail when @auditaur/api has shipped-code changes that no published api release covers.

The CLI and plugin release through release-plz; @auditaur/api releases separately through
release-please. A Rust release that depends on unreleased frontend bridge changes leaves apps
with nothing to upgrade to, so the Rust release gates on this check.

`--published-version` prints the npm version matching packages/api/src (or nothing), for the
CI job that runs drills against the published api.
"""

import importlib.util
import json
import pathlib
import shutil
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.dont_write_bytecode = True
_spec = importlib.util.spec_from_file_location("compat", ROOT / "scripts/check-compat.py")
compat = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(compat)
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


def published_version() -> int:
    """Print the npm api version that matches packages/api/src, or nothing when none does.

    CI drives the dogfood app with this published tarball to prove the CLI works with what
    users install. It prints nothing (and exits 0) while the source has unreleased changes,
    because then no published api matches the drills on this branch.
    """
    version = json.loads((ROOT / ".release-please-manifest.json").read_text(encoding="utf-8"))["packages/api"]
    tag = f"api-v{version}"
    log = unreleased_commits(tag)
    if log is None:
        print(f"Cannot read history since {tag}; fetch tags (fetch-depth: 0).", file=sys.stderr)
        return 1
    if log:
        print(f"Skipping: packages/api/src changed since {tag}, so no published api matches it.", file=sys.stderr)
        return 0
    npm = shutil.which("npm")
    if npm is None:
        sys.exit("npm is required to check the published @auditaur/api version.")
    # Only a confirmed 404 skips; a registry outage must not quietly drop the published leg.
    for attempt in range(3):
        result = subprocess.run(
            [npm, "view", f"{PACKAGE}@{version}", "version", "--prefer-online"],
            capture_output=True,
            text=True,
        )
        if result.returncode == 0 and result.stdout.strip() == version:
            print(version)
            return 0
        if "E404" in result.stderr:
            print(f"Skipping: {PACKAGE}@{version} is tagged but not on npm yet.", file=sys.stderr)
            return 0
        if attempt < 2:
            time.sleep(15)
    print(f"Cannot read {PACKAGE}@{version} from npm:\n{result.stderr.strip()}", file=sys.stderr)
    return 1


def main() -> int:
    if sys.argv[1:] == ["--published-version"]:
        return published_version()
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

    # The CLI refuses older bridges, so the version it requires must be installable (AR-1).
    required = compat.min_drill_api_version()
    if required != version and not published(required):
        print(f"The CLI requires {PACKAGE}@{required} (MIN_DRILL_API_VERSION), which is not on npm.", file=sys.stderr)
        return 1

    print(f"{PACKAGE}@{version} is published and covers packages/api/src; drills require {required}.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
