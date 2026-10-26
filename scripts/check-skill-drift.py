from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parents[1]
REPO_SKILL = ROOT / ".github" / "skills" / "auditaur-debug" / "SKILL.md"
CLI_SKILL_ASSET = ROOT / "crates" / "auditaur-cli" / "assets" / "auditaur-debug-skill.md"

REPO_DRILLS_EXTENSION = ROOT / ".github" / "extensions" / "auditaur-drills"
CLI_DRILLS_EXTENSION_ASSET = ROOT / "crates" / "auditaur-cli" / "assets" / "auditaur-drills"


def normalized(path: Path) -> str:
    return path.read_text(encoding="utf-8").replace("\r\n", "\n")


def files_under(path: Path) -> set[str]:
    return {p.relative_to(path).as_posix() for p in path.rglob("*") if p.is_file()}


def expand(pairs):
    """Turns directory pairs into per-file pairs; returns them plus any file-set mismatches."""
    files, mismatches = [], []
    for label, repo_path, asset_path in pairs:
        if not repo_path.is_dir():
            files.append((label, repo_path, asset_path))
            continue
        repo_files, asset_files = files_under(repo_path), files_under(asset_path)
        for name in sorted(repo_files ^ asset_files):
            owner = repo_path if name in repo_files else asset_path
            mismatches.append(f"{label}: {(owner / name).relative_to(ROOT)} has no counterpart")
        files += [(label, repo_path / name, asset_path / name) for name in sorted(repo_files & asset_files)]
    return files, mismatches


def main() -> int:
    pairs = [
        (
            "Auditaur debug skill",
            REPO_SKILL,
            CLI_SKILL_ASSET,
        ),

        (
            "Auditaur drills canvas extension",
            REPO_DRILLS_EXTENSION,
            CLI_DRILLS_EXTENSION_ASSET,
        ),
    ]
    missing = [path for _, repo_path, asset_path in pairs for path in (repo_path, asset_path) if not path.exists()]
    if missing:
        for path in missing:
            print(f"missing required packaged file: {path.relative_to(ROOT)}", file=sys.stderr)
        return 1

    pairs, mismatches = expand(pairs)
    if mismatches:
        for mismatch in mismatches:
            print(f"{mismatch}. Keep the packaged copy in sync.", file=sys.stderr)
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
