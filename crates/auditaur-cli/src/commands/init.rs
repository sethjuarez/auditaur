use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;

use crate::commands::read;

pub(crate) const AUDITAUR_DEBUG_SKILL: &str = include_str!("../../assets/auditaur-debug-skill.md");
pub(crate) const AUDITAUR_GATE_EXTENSION: &str =
    include_str!("../../assets/auditaur-gate-extension.mjs");
const GITHUB_SKILL_RELATIVE_PATH: [&str; 4] = [".github", "skills", "auditaur-debug", "SKILL.md"];
const AGENTS_SKILL_RELATIVE_PATH: [&str; 4] = [".agents", "skills", "auditaur-debug", "SKILL.md"];
const GITHUB_EXTENSION_RELATIVE_DIR: [&str; 3] = [".github", "extensions", "auditaur-gate"];
const AUDITAUR_GATE_EXTENSION_FILES: &[(&str, &str)] =
    &[("extension.mjs", AUDITAUR_GATE_EXTENSION)];
const GITHUB_DRILLS_EXTENSION_RELATIVE_DIR: [&str; 3] =
    [".github", "extensions", "auditaur-drills"];

/// Embeds files from `assets/auditaur-drills` under their path relative to the extension folder.
macro_rules! drills_extension_files {
    ($($path:literal,)*) => {
        &[$(($path, include_str!(concat!("../../assets/auditaur-drills/", $path))),)*]
    };
}

/// Every file of the drills canvas extension. The entry point comes first.
pub(crate) const AUDITAUR_DRILLS_EXTENSION_FILES: &[(&str, &str)] = drills_extension_files![
    "extension.mjs",
    "canvases/coverage/index.mjs",
    "canvases/coverage/view.mjs",
    "canvases/explorer/index.mjs",
    "canvases/explorer/view.mjs",
    "canvases/live/index.mjs",
    "canvases/live/view.mjs",
    "canvases/snapshots/index.mjs",
    "canvases/snapshots/view.mjs",
    "canvases/trace/index.mjs",
    "canvases/trace/view.mjs",
    "lib/apps.mjs",
    "lib/attachments.mjs",
    "lib/baselines.mjs",
    "lib/cli.mjs",
    "lib/errors.mjs",
    "lib/events.mjs",
    "lib/files.mjs",
    "lib/gates.mjs",
    "lib/launcher.mjs",
    "lib/panels.mjs",
    "lib/paths.mjs",
    "lib/processes.mjs",
    "lib/runs.mjs",
    "lib/schemas.mjs",
    "lib/server.mjs",
    "lib/telemetry.mjs",
    "ui/base.css",
    "ui/checklist.css",
    "ui/checklist.js",
    "ui/common.js",
    "ui/compare.css",
    "ui/compare.js",
    "ui/coverage.css",
    "ui/coverage.html",
    "ui/coverage.js",
    "ui/explorer.css",
    "ui/explorer.html",
    "ui/explorer.js",
    "ui/live.css",
    "ui/live.html",
    "ui/live.js",
    "ui/matrix.css",
    "ui/matrix.js",
    "ui/meter.css",
    "ui/meter.js",
    "ui/segmented.css",
    "ui/segmented.js",
    "ui/snapshots.css",
    "ui/snapshots.html",
    "ui/snapshots.js",
    "ui/trace.css",
    "ui/trace.html",
    "ui/trace.js",
];
const DIAGNOSTICS_CONFIG_RELATIVE_PATH: [&str; 2] = [".auditaur", "diagnostics.json"];
const DIAGNOSTICS_GUIDE_RELATIVE_PATH: [&str; 2] = [".auditaur", "diagnostics.md"];
const DIAGNOSTICS_CONFIG: &str = r#"{
  "version": 1,
  "checkpointConvention": "<domain_or_operation>.<phase>",
  "phases": ["started", "succeeded", "failed", "retrying", "cancelled"],
  "signals": {
    "failures": {
      "description": "Generic failure signals over existing observed telemetry.",
      "includes": ["frontend_error", "failed_ipc", "failed_span", "error_log", "panic"]
    }
  },
  "privacy": {
    "doNotRecord": [
      "tokens",
      "authorization headers",
      "cookies",
      "passwords",
      "URLs with secrets",
      "provider frames",
      "raw audio or video",
      "full prompts",
      "transcripts",
      "user content"
    ]
  }
}
"#;
const DIAGNOSTICS_GUIDE: &str = r#"# Auditaur diagnostics guidance

Use pinned run files for follow-up diagnostics:

```powershell
auditaur observe --app <app-name> --write-session .auditaur/session.json -- <dev command>
auditaur diagnose --session-file .auditaur/session.json
auditaur timeline --session-file .auditaur/session.json --anchor error:latest --window 10s
auditaur related --session-file .auditaur/session.json --anchor ipc:<command> --anchor-window 10s
auditaur tail --session-file .auditaur/session.json --signal failures --replay
```

If the app emits semantic checkpoints, prefer names like `<domain_or_operation>.<phase>`,
for example `settings.save.started`, `settings.save.failed`, or `sync.retrying`.
Checkpoint attributes must be privacy-safe structured values only; never include secrets,
raw media, full prompts, transcripts, or user content.

Diagnostics config and guidance help future instrumentation stay safe, but they do not
retroactively redact raw application logs. If the app logs provider frames, prompts,
transcripts, URLs with secrets, or user content, Auditaur may surface those log lines in
local timelines and JSON output. Redact at the app logging boundary before recording.
"#;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InitSkillResult {
    ok: bool,
    path: String,
    overwritten: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InitExtensionResult {
    ok: bool,
    /// The gate extension's path, kept for callers written before multiple extensions.
    path: String,
    overwritten: bool,
    extensions: Vec<InitExtension>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InitExtension {
    name: &'static str,
    path: String,
    status: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InitDiagnosticsResult {
    ok: bool,
    dry_run: bool,
    files: Vec<InitDiagnosticsFile>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InitDiagnosticsFile {
    path: String,
    existed: bool,
    written: bool,
    overwritten: bool,
}

pub fn skill(path: Option<&Path>, force: bool, agents_path: bool, json: bool) -> Result<()> {
    let root = path
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir().context("failed to resolve current directory")?);
    let relative_path = if agents_path {
        AGENTS_SKILL_RELATIVE_PATH
    } else {
        GITHUB_SKILL_RELATIVE_PATH
    };
    let output = relative_path
        .iter()
        .fold(root, |path, segment| path.join(segment));
    let existed = output.exists();
    if existed && !force {
        return Err(anyhow!(
            "Auditaur debug skill already exists at {}. Pass --force to overwrite it.",
            output.display()
        ));
    }

    let parent = output
        .parent()
        .ok_or_else(|| anyhow!("invalid skill output path: {}", output.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create skill directory {}", parent.display()))?;
    fs::write(&output, AUDITAUR_DEBUG_SKILL)
        .with_context(|| format!("failed to write skill file {}", output.display()))?;
    let result = InitSkillResult {
        ok: true,
        path: output.to_string_lossy().to_string(),
        overwritten: existed,
    };
    read::print_json_or_table(json, &result, || {
        println!("Installed Auditaur debug skill at {}", output.display());
        if existed {
            println!("Existing skill was overwritten.");
        }
        Ok(())
    })
}

/// Installs every packaged canvas extension. Without `--force`, extensions that already exist
/// are left alone and only missing ones are written; it fails only when nothing is missing.
pub fn extension(path: Option<&Path>, force: bool, json: bool) -> Result<()> {
    let root = path
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir().context("failed to resolve current directory")?);
    let targets = [
        (
            "auditaur-gate",
            GITHUB_EXTENSION_RELATIVE_DIR,
            AUDITAUR_GATE_EXTENSION_FILES,
        ),
        (
            "auditaur-drills",
            GITHUB_DRILLS_EXTENSION_RELATIVE_DIR,
            AUDITAUR_DRILLS_EXTENSION_FILES,
        ),
    ]
    .map(|(name, relative, files)| {
        let dir = relative
            .iter()
            .fold(root.clone(), |path, segment| path.join(segment));
        let output = dir.join("extension.mjs");
        let present = files
            .iter()
            .filter(|(file, _)| file_path(&dir, file).is_file())
            .count();
        (
            name,
            output,
            (dir, files),
            (present == files.len(), present > 0),
        )
    });
    if !force && targets.iter().all(|(_, _, _, (complete, _))| *complete) {
        return Err(anyhow!(
            "Auditaur canvas extensions already exist at {}. Pass --force to overwrite them.",
            targets
                .iter()
                .map(|(_, output, _, _)| output.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let mut extensions = Vec::new();
    for (name, output, (dir, files), (complete, partial)) in &targets {
        // Missing files mean an interrupted or older install: without --force, only those
        // files are written so local edits survive.
        let status = match (complete, partial, force) {
            (true, _, false) => "skipped",
            (_, true, true) => "overwritten",
            (false, true, false) => "repaired",
            (_, false, _) => "installed",
        };
        if status != "skipped" {
            write_extension_files(dir, files, force)?;
        }
        extensions.push(InitExtension {
            name,
            path: output.to_string_lossy().to_string(),
            status,
        });
    }
    let result = InitExtensionResult {
        ok: true,
        path: extensions[0].path.clone(),
        overwritten: extensions[0].status == "overwritten",
        extensions,
    };
    read::print_json_or_table(json, &result, || {
        for ext in &result.extensions {
            match ext.status {
                "installed" => println!("Installed {} canvas extension at {}", ext.name, ext.path),
                "overwritten" => {
                    println!("Overwrote {} canvas extension at {}", ext.name, ext.path)
                }
                "repaired" => println!(
                    "Restored missing files of the {} canvas extension at {}",
                    ext.name, ext.path
                ),
                _ => println!(
                    "Kept existing {} canvas extension at {} (pass --force to overwrite)",
                    ext.name, ext.path
                ),
            }
        }
        Ok(())
    })
}

fn file_path(dir: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(dir.to_path_buf(), |path, segment| path.join(segment))
}

/// Writes the entry point last, so an interrupted install never looks complete.
fn write_extension_files(dir: &Path, files: &[(&str, &str)], overwrite: bool) -> Result<()> {
    for (relative, contents) in files.iter().rev() {
        let output = file_path(dir, relative);
        if !overwrite && output.is_file() {
            continue;
        }
        let parent = output
            .parent()
            .ok_or_else(|| anyhow!("invalid extension output path: {}", output.display()))?;
        fs::create_dir_all(parent).with_context(|| {
            format!("failed to create extension directory {}", parent.display())
        })?;
        fs::write(&output, contents)
            .with_context(|| format!("failed to write extension file {}", output.display()))?;
    }
    Ok(())
}

pub fn diagnostics(path: Option<&Path>, force: bool, dry_run: bool, json: bool) -> Result<()> {
    let root = path
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir().context("failed to resolve current directory")?);
    let files = [
        (
            DIAGNOSTICS_CONFIG_RELATIVE_PATH
                .iter()
                .fold(root.clone(), |path, segment| path.join(segment)),
            DIAGNOSTICS_CONFIG,
        ),
        (
            DIAGNOSTICS_GUIDE_RELATIVE_PATH
                .iter()
                .fold(root, |path, segment| path.join(segment)),
            DIAGNOSTICS_GUIDE,
        ),
    ];

    if !force && !dry_run {
        for (output, _) in &files {
            if output.exists() {
                return Err(anyhow!(
                    "Auditaur diagnostics file already exists at {}. Pass --force to overwrite it.",
                    output.display()
                ));
            }
        }
    }

    let mut result_files = Vec::new();
    for (output, contents) in files {
        let existed = output.exists();
        if !dry_run {
            let parent = output
                .parent()
                .ok_or_else(|| anyhow!("invalid diagnostics output path: {}", output.display()))?;
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create diagnostics directory {}",
                    parent.display()
                )
            })?;
            if !existed || force {
                fs::write(&output, contents).with_context(|| {
                    format!("failed to write diagnostics file {}", output.display())
                })?;
            }
        }
        result_files.push(InitDiagnosticsFile {
            path: output.to_string_lossy().to_string(),
            existed,
            written: !dry_run && (!existed || force),
            overwritten: !dry_run && existed && force,
        });
    }

    let result = InitDiagnosticsResult {
        ok: true,
        dry_run,
        files: result_files,
    };
    read::print_json_or_table(json, &result, || {
        for file in &result.files {
            let action = if dry_run && file.existed && !force {
                "exists; would leave unchanged"
            } else if dry_run && file.existed && force {
                "exists; would overwrite"
            } else if dry_run {
                "missing; would write"
            } else if file.overwritten {
                "overwrote"
            } else if file.written {
                "wrote"
            } else {
                "kept"
            };
            println!("{action} {}", file.path);
        }
        Ok(())
    })
}

pub fn run(args: &[String]) -> Result<()> {
    let Some(command) = args.first() else {
        print_help();
        return Ok(());
    };
    if command == "--help" || command == "-h" {
        print_help();
        return Ok(());
    }
    if command != "skill" && command != "extension" && command != "diagnostics" {
        return Err(anyhow!(
            "unknown init command '{}'. Expected 'skill', 'extension', or 'diagnostics'.",
            command
        ));
    }

    let mut path = None;
    let mut force = false;
    let mut agents_path = false;
    let mut dry_run = false;
    let mut json = false;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--path" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| anyhow!("--path requires a value"))?;
                path = Some(PathBuf::from(value));
            }
            "--force" => force = true,
            "--dry-run" => dry_run = true,
            "--agents-path" => agents_path = true,
            "--json" => json = true,
            "--help" | "-h" => {
                if command == "skill" {
                    print_skill_help();
                } else if command == "diagnostics" {
                    print_diagnostics_help();
                } else {
                    print_extension_help();
                }
                return Ok(());
            }
            unknown => return Err(anyhow!("unknown init {} option '{}'", command, unknown)),
        }
        index += 1;
    }
    match command.as_str() {
        "skill" => {
            if dry_run {
                return Err(anyhow!("--dry-run is only supported by init diagnostics"));
            }
            skill(path.as_deref(), force, agents_path, json)
        }
        "extension" => {
            if agents_path {
                return Err(anyhow!("--agents-path is only supported by init skill"));
            }
            if dry_run {
                return Err(anyhow!("--dry-run is only supported by init diagnostics"));
            }
            extension(path.as_deref(), force, json)
        }
        "diagnostics" => {
            if agents_path {
                return Err(anyhow!("--agents-path is only supported by init skill"));
            }
            diagnostics(path.as_deref(), force, dry_run, json)
        }
        _ => unreachable!("validated init command"),
    }
}

fn print_help() {
    println!("Usage: auditaur init <COMMAND>");
    println!();
    println!("Commands:");
    println!("  skill  Install the Auditaur debug agent skill into a repository");
    println!("  extension  Install the Auditaur canvas extensions (manual gate, drill explorer) into a repository");
    println!("  diagnostics  Install optional generic diagnostics config and guidance");
}

fn print_skill_help() {
    println!("Usage: auditaur init skill [--path <repo-root>] [--agents-path] [--force] [--json]");
}

fn print_extension_help() {
    println!("Usage: auditaur init extension [--path <repo-root>] [--force] [--json]");
    println!();
    println!("Installs .github/extensions/auditaur-gate and .github/extensions/auditaur-drills.");
    println!("Existing extensions are kept unless --force is passed.");
}

fn print_diagnostics_help() {
    println!(
        "Usage: auditaur init diagnostics [--path <repo-root>] [--dry-run] [--force] [--json]"
    );
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::AUDITAUR_DRILLS_EXTENSION_FILES;

    fn tree(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                tree(root, &path, out);
            } else {
                let relative = path.strip_prefix(root).unwrap();
                out.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }

    #[test]
    fn drills_extension_file_list_matches_packaged_assets() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/auditaur-drills");
        let mut on_disk = Vec::new();
        tree(&root, &root, &mut on_disk);
        on_disk.sort();
        let mut listed: Vec<String> = AUDITAUR_DRILLS_EXTENSION_FILES
            .iter()
            .map(|(path, _)| path.to_string())
            .collect();
        assert_eq!(listed[0], "extension.mjs");
        listed.sort();
        assert_eq!(listed, on_disk);
    }
}
