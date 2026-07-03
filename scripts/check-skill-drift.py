from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parents[1]
REPO_SKILL = ROOT / ".github" / "skills" / "auditaur-debug" / "SKILL.md"
CLI_SKILL_ASSET = ROOT / "crates" / "auditaur-cli" / "assets" / "auditaur-debug-skill.md"
REPO_EXTENSION = ROOT / ".github" / "extensions" / "auditaur-gate" / "extension.mjs"
CLI_EXTENSION_ASSET = ROOT / "crates" / "auditaur-cli" / "assets" / "auditaur-gate-extension.mjs"


def normalized(path: Path) -> str:
    return path.read_text(encoding="utf-8").replace("\r\n", "\n")


def main() -> int:
    pairs = [
        (
            "Auditaur debug skill",
            REPO_SKILL,
            CLI_SKILL_ASSET,
        ),
        (
            "Auditaur gate canvas extension",
            REPO_EXTENSION,
            CLI_EXTENSION_ASSET,
        ),
    ]
    missing = [path for _, repo_path, asset_path in pairs for path in (repo_path, asset_path) if not path.exists()]
    if missing:
        for path in missing:
            print(f"missing required packaged file: {path.relative_to(ROOT)}", file=sys.stderr)
        return 1

    for label, repo_path, asset_path in pairs:
        if normalized(repo_path) != normalized(asset_path):
            print(
                f"{label} drift detected. "
                f"Keep {repo_path.relative_to(ROOT)} and {asset_path.relative_to(ROOT)} identical.",
                file=sys.stderr,
            )
            return 1

    print("Auditaur packaged skill and extension files match.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
