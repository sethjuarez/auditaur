from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parents[1]
REPO_SKILL = ROOT / ".github" / "skills" / "auditaur-debug" / "SKILL.md"
CLI_ASSET = ROOT / "crates" / "auditaur-cli" / "assets" / "auditaur-debug-skill.md"


def normalized(path: Path) -> str:
    return path.read_text(encoding="utf-8").replace("\r\n", "\n")


def main() -> int:
    missing = [path for path in (REPO_SKILL, CLI_ASSET) if not path.exists()]
    if missing:
        for path in missing:
            print(f"missing required skill file: {path.relative_to(ROOT)}", file=sys.stderr)
        return 1

    if normalized(REPO_SKILL) != normalized(CLI_ASSET):
        print(
            "Auditaur debug skill drift detected. "
            f"Keep {REPO_SKILL.relative_to(ROOT)} and {CLI_ASSET.relative_to(ROOT)} identical.",
            file=sys.stderr,
        )
        return 1

    print("Auditaur debug skill files match.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
