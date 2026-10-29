"""Keeps every packaged copy of the Auditaur skill and drills canvas identical to its source.

Sources live under `.github/`. Copies ship in the CLI crate (`auditaur init`) and in the Copilot
plugin (`plugins/auditaur`). The plugin and marketplace versions follow the workspace crate version.

    python scripts/check-skill-drift.py          # fail on any drift
    python scripts/check-skill-drift.py --write  # regenerate the copies and stamp versions
"""

from pathlib import Path
import json
import re
import shutil
import sys


ROOT = Path(__file__).resolve().parents[1]
REPO_SKILL = ROOT / ".github" / "skills" / "auditaur-debug" / "SKILL.md"
REPO_DRILLS_EXTENSION = ROOT / ".github" / "extensions" / "auditaur-drills"

CLI_SKILL_ASSET = ROOT / "crates" / "auditaur-cli" / "assets" / "auditaur-debug-skill.md"
CLI_DRILLS_EXTENSION_ASSET = ROOT / "crates" / "auditaur-cli" / "assets" / "auditaur-drills"

PLUGIN = ROOT / "plugins" / "auditaur"
PLUGIN_MANIFEST = PLUGIN / "plugin.json"
PLUGIN_SKILL = PLUGIN / "skills" / "auditaur-debug" / "SKILL.md"
PLUGIN_DRILLS_EXTENSION = PLUGIN / "com.github.copilot" / "extensions" / "auditaur-drills"
MARKETPLACE = ROOT / ".github" / "plugin" / "marketplace.json"

COPIES = [
    ("Auditaur debug skill", REPO_SKILL, CLI_SKILL_ASSET),
    ("Auditaur drills canvas extension", REPO_DRILLS_EXTENSION, CLI_DRILLS_EXTENSION_ASSET),
    ("Auditaur plugin skill", REPO_SKILL, PLUGIN_SKILL),
    ("Auditaur plugin drills canvas", REPO_DRILLS_EXTENSION, PLUGIN_DRILLS_EXTENSION),
]


def normalized(path: Path) -> str:
    return path.read_text(encoding="utf-8").replace("\r\n", "\n")


def files_under(path: Path) -> set[str]:
    return {p.relative_to(path).as_posix() for p in path.rglob("*") if p.is_file()}


def workspace_version() -> str:
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    section = re.search(r"^\[workspace\.package\]\s*$(.*?)(?=^\[|\Z)", text, re.M | re.S)
    match = section and re.search(r'^version\s*=\s*"([^"]+)"', section.group(1), re.M)
    if not match:
        raise SystemExit("Cargo.toml has no [workspace.package] version")
    return match.group(1)


def marketplace_entry(marketplace: dict) -> dict:
    entries = [p for p in marketplace.get("plugins", []) if p.get("name") == "auditaur"]
    if len(entries) != 1:
        raise SystemExit(f"{MARKETPLACE.relative_to(ROOT)} must list the auditaur plugin exactly once")
    return entries[0]


def check_copies() -> list[str]:
    problems = []
    for label, source, copy in COPIES:
        missing = [path for path in (source, copy) if not path.exists()]
        problems += [f"missing required packaged file: {path.relative_to(ROOT)}" for path in missing]
        if missing:
            continue
        if source.is_dir():
            source_files, copy_files = files_under(source), files_under(copy)
            for name in sorted(source_files ^ copy_files):
                owner = source if name in source_files else copy
                problems.append(f"{label}: {(owner / name).relative_to(ROOT)} has no counterpart")
            pairs = [(source / name, copy / name) for name in sorted(source_files & copy_files)]
        else:
            pairs = [(source, copy)]
        for a, b in pairs:
            if normalized(a) != normalized(b):
                problems.append(
                    f"{label} drift: keep {a.relative_to(ROOT)} and {b.relative_to(ROOT)} identical"
                )
    return problems


def check_versions(version: str) -> list[str]:
    problems = []
    for path in (PLUGIN_MANIFEST, MARKETPLACE):
        if not path.exists():
            return [f"missing required packaged file: {path.relative_to(ROOT)}"]
    manifest = json.loads(PLUGIN_MANIFEST.read_text(encoding="utf-8"))
    entry = marketplace_entry(json.loads(MARKETPLACE.read_text(encoding="utf-8")))
    for label, found in (
        (PLUGIN_MANIFEST.relative_to(ROOT).as_posix(), manifest.get("version")),
        (f"{MARKETPLACE.relative_to(ROOT).as_posix()} (auditaur entry)", entry.get("version")),
    ):
        if found != version:
            problems.append(f"{label} version {found} does not match the workspace version {version}")
    return problems


def write_json(path: Path, data: dict) -> None:
    path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")


def write(version: str) -> None:
    for _, source, copy in COPIES:
        if source.is_dir():
            shutil.rmtree(copy, ignore_errors=True)
            shutil.copytree(source, copy)
        else:
            copy.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, copy)
    manifest = json.loads(PLUGIN_MANIFEST.read_text(encoding="utf-8"))
    manifest["version"] = version
    write_json(PLUGIN_MANIFEST, manifest)
    marketplace = json.loads(MARKETPLACE.read_text(encoding="utf-8"))
    marketplace_entry(marketplace)["version"] = version
    write_json(MARKETPLACE, marketplace)


def main(argv: list[str]) -> int:
    version = workspace_version()
    if "--write" in argv:
        write(version)
        print(f"Regenerated packaged copies; plugin version {version}.")
    problems = check_copies() + check_versions(version)
    for problem in problems:
        print(f"{problem}. Run `python scripts/check-skill-drift.py --write`.", file=sys.stderr)
    if problems:
        return 1
    print(f"Auditaur packaged skill, extension, and plugin {version} match.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
