use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;

use crate::commands::read;

const AUDITAUR_DEBUG_SKILL: &str =
    include_str!("../../../../.github/skills/auditaur-debug/SKILL.md");
const SKILL_RELATIVE_PATH: [&str; 4] = [".github", "skills", "auditaur-debug", "SKILL.md"];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InitSkillResult {
    ok: bool,
    path: String,
    overwritten: bool,
}

pub fn skill(path: Option<&Path>, force: bool, json: bool) -> Result<()> {
    let root = path
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir().context("failed to resolve current directory")?);
    let output = SKILL_RELATIVE_PATH
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

pub fn run(args: &[String]) -> Result<()> {
    let Some(command) = args.first() else {
        print_help();
        return Ok(());
    };
    if command == "--help" || command == "-h" {
        print_help();
        return Ok(());
    }
    if command != "skill" {
        return Err(anyhow!(
            "unknown init command '{}'. Expected 'skill'.",
            command
        ));
    }

    let mut path = None;
    let mut force = false;
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
            "--json" => json = true,
            "--help" | "-h" => {
                print_skill_help();
                return Ok(());
            }
            unknown => return Err(anyhow!("unknown init skill option '{}'", unknown)),
        }
        index += 1;
    }
    skill(path.as_deref(), force, json)
}

fn print_help() {
    println!("Usage: auditaur init <COMMAND>");
    println!();
    println!("Commands:");
    println!("  skill  Install the Auditaur debug agent skill into a repository");
}

fn print_skill_help() {
    println!("Usage: auditaur init skill [--path <repo-root>] [--force] [--json]");
}
