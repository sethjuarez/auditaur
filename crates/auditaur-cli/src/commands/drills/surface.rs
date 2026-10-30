//! The app's registered IPC command surface, so coverage can list commands no test called.
//!
//! Tauri's `generate_handler!` builds an opaque closure: the running app cannot report which
//! commands it registered. So the surface is read from where it is declared:
//! - Rust sources (a file or a folder): every name listed in `generate_handler![...]`.
//! - A `.json` file: an array of command names.
//! - Any other file: one command name per line; `#` starts a comment.
//!
//! Paths come from `--commands`, else `coverage.commands` in `.auditaur/config.json`, else
//! `src-tauri/src` when that folder exists.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::Serialize;

/// Where Tauri apps keep their Rust sources by default.
pub const DEFAULT_SURFACE: &str = "src-tauri/src";

/// One surface path and what it listed.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceSummary {
    pub path: String,
    pub commands: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The surface paths to read: `--commands`, else the config's `coverage.commands` (relative
/// to the workspace), else [`DEFAULT_SURFACE`] when it exists. A config that cannot be read
/// is reported as an unreadable surface rather than silently falling back.
pub fn paths(
    workspace: &Path,
    flags: &[PathBuf],
    config: &Path,
) -> Vec<std::result::Result<PathBuf, SurfaceSummary>> {
    if !flags.is_empty() {
        return flags.iter().cloned().map(Ok).collect();
    }
    let config_path = workspace.join(config);
    match configured(&config_path) {
        Ok(Some(listed)) => listed
            .into_iter()
            .map(|path| Ok(workspace.join(path)))
            .collect(),
        Ok(None) => {
            let default = workspace.join(DEFAULT_SURFACE);
            if default.is_dir() {
                vec![Ok(default)]
            } else {
                Vec::new()
            }
        }
        Err(error) => vec![Err(SurfaceSummary {
            path: config.display().to_string(),
            commands: 0,
            error: Some(format!("{error:#}")),
        })],
    }
}

/// `coverage.commands` from the config, when the config and the key exist.
fn configured(path: &Path) -> Result<Option<Vec<String>>> {
    if !path.is_file() {
        return Ok(None);
    }
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read `{}`", path.display()))?;
    let config: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse `{}`", path.display()))?;
    let Some(listed) = config.pointer("/coverage/commands") else {
        return Ok(None);
    };
    let listed: Vec<String> = serde_json::from_value(listed.clone()).with_context(|| {
        format!(
            "`coverage.commands` in `{}` must be an array of paths",
            path.display()
        )
    })?;
    Ok(Some(listed))
}

/// Read every surface path. Unreadable ones are reported in their summary, not fatal.
pub fn read(
    paths: Vec<std::result::Result<PathBuf, SurfaceSummary>>,
) -> (BTreeSet<String>, Vec<SurfaceSummary>) {
    let mut registered = BTreeSet::new();
    let mut summaries = Vec::new();
    for path in paths {
        let path = match path {
            Ok(path) => path,
            Err(summary) => {
                summaries.push(summary);
                continue;
            }
        };
        let mut summary = SurfaceSummary {
            path: path.display().to_string(),
            commands: 0,
            error: None,
        };
        match read_path(&path) {
            Ok(names) => {
                summary.commands = names.len();
                registered.extend(names);
            }
            Err(error) => summary.error = Some(format!("{error:#}")),
        }
        summaries.push(summary);
    }
    (registered, summaries)
}

fn read_path(path: &Path) -> Result<BTreeSet<String>> {
    if path.is_dir() {
        let mut files = Vec::new();
        rust_files(path, &mut files)?;
        let mut names = BTreeSet::new();
        for file in files {
            names.extend(handler_names(&read_text(&file)?));
        }
        if names.is_empty() {
            anyhow::bail!("no `generate_handler!` commands under `{}`", path.display());
        }
        return Ok(names);
    }
    let text = read_text(path)?;
    let names: BTreeSet<String> = match path.extension().and_then(|ext| ext.to_str()) {
        Some("rs") => handler_names(&text).into_iter().collect(),
        Some("json") => serde_json::from_str::<Vec<String>>(&text)
            .with_context(|| format!("`{}` must be an array of command names", path.display()))?
            .into_iter()
            .collect(),
        _ => text
            .lines()
            .map(|line| line.split('#').next().unwrap_or("").trim())
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect(),
    };
    if names.is_empty() {
        anyhow::bail!("no commands listed in `{}`", path.display());
    }
    Ok(names)
}

fn read_text(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("failed to read `{}`", path.display()))
}

/// Every `.rs` file under `dir`, skipping build output and hidden folders, in a stable order.
fn rust_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .with_context(|| format!("failed to read `{}`", dir.display()))?
        .filter_map(|entry| entry.ok())
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name != "target" && name != "node_modules" && !name.starts_with('.') {
                rust_files(&path, files)?;
            }
        } else if name.ends_with(".rs") {
            files.push(path);
        }
    }
    Ok(())
}

/// The command names listed in every `generate_handler!` call in Rust source: the last path
/// segment of each entry (`commands::save` registers `save`), skipping `#[cfg(...)]`-style
/// attributes and comments.
pub fn handler_names(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = source;
    while let Some(at) = rest.find("generate_handler!") {
        rest = &rest[at + "generate_handler!".len()..];
        let Some((body, after)) = delimited(rest) else {
            continue;
        };
        rest = after;
        for entry in split_entries(&body) {
            let path = entry.trim();
            let name = path.rsplit("::").next().unwrap_or(path).trim();
            if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                names.push(name.to_string());
            }
        }
    }
    names
}

/// The text inside the bracket that opens `text` (after whitespace), with comments removed,
/// and the text after its matching close.
fn delimited(text: &str) -> Option<(String, &str)> {
    let start = text.len() - text.trim_start().len();
    let open = text[start..].chars().next()?;
    let close = match open {
        '[' => ']',
        '(' => ')',
        '{' => '}',
        _ => return None,
    };
    let mut depth = 0usize;
    let mut body = String::new();
    let mut chars = text[start..].char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if c == '/' && chars.peek().map(|(_, next)| *next) == Some('/') {
            for (_, c) in chars.by_ref() {
                if c == '\n' {
                    break;
                }
            }
            body.push('\n');
            continue;
        }
        if c == '/' && chars.peek().map(|(_, next)| *next) == Some('*') {
            chars.next();
            let mut previous = ' ';
            for (_, c) in chars.by_ref() {
                if previous == '*' && c == '/' {
                    break;
                }
                previous = c;
            }
            body.push(' ');
            continue;
        }
        if c == open {
            depth += 1;
            if depth == 1 {
                continue;
            }
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return Some((body, &text[start + index + c.len_utf8()..]));
            }
        }
        body.push(c);
    }
    None
}

/// Top-level comma-separated entries, with `#[...]` attributes removed.
fn split_entries(body: &str) -> Vec<String> {
    let mut entries = vec![String::new()];
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '#' && chars.peek() == Some(&'[') {
            let mut depth = 0usize;
            for c in chars.by_ref() {
                match c {
                    '[' => depth += 1,
                    ']' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            continue;
        }
        if c == ',' {
            entries.push(String::new());
        } else {
            entries.last_mut().unwrap().push(c);
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_names_from_generate_handler_calls() {
        let source = r#"
            fn main() {
                tauri::Builder::default()
                    .invoke_handler(tauri::generate_handler![
                        greet,
                        commands::chat::save_chat_session, // the transcript
                        /* list_old, */ list_variations,
                        #[cfg(debug_assertions)]
                        dev_only,
                    ])
                    .run(tauri::generate_context!());
            }
            fn other() { let _ = generate_handler!(a::b::c); }
        "#;
        assert_eq!(
            handler_names(source),
            [
                "greet",
                "save_chat_session",
                "list_variations",
                "dev_only",
                "c"
            ]
        );
    }

    #[test]
    fn ignores_mentions_without_a_list() {
        assert!(handler_names("// generate_handler! is opaque").is_empty());
        assert!(handler_names("generate_handler![").is_empty());
    }

    #[test]
    fn reads_folders_lists_and_json() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src-tauri/src");
        fs::create_dir_all(src.join("commands")).unwrap();
        fs::create_dir_all(src.join("target")).unwrap();
        fs::write(
            src.join("lib.rs"),
            "fn run() { generate_handler![a, commands::b]; }",
        )
        .unwrap();
        fs::write(src.join("commands/mod.rs"), "// no handlers here").unwrap();
        fs::write(src.join("target/gen.rs"), "generate_handler![built];").unwrap();
        fs::write(
            dir.path().join("list.txt"),
            "c\n# comment\n\nd # trailing\n",
        )
        .unwrap();
        fs::write(dir.path().join("list.json"), r#"["e"]"#).unwrap();
        fs::write(dir.path().join("empty.json"), "[]").unwrap();

        let (registered, summaries) = read(vec![
            Ok(src.clone()),
            Ok(dir.path().join("list.txt")),
            Ok(dir.path().join("list.json")),
            Ok(dir.path().join("empty.json")),
            Ok(dir.path().join("missing.txt")),
        ]);

        assert_eq!(
            registered.into_iter().collect::<Vec<_>>(),
            ["a", "b", "c", "d", "e"]
        );
        let counts: Vec<_> = summaries.iter().map(|s| s.commands).collect();
        assert_eq!(counts, [2, 2, 1, 0, 0]);
        assert!(summaries[3]
            .error
            .as_deref()
            .unwrap()
            .contains("no commands"));
        assert!(summaries[4].error.is_some());
    }

    #[test]
    fn paths_prefer_flags_then_config_then_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let config = Path::new(".auditaur/config.json");
        assert!(paths(dir.path(), &[], config).is_empty());

        fs::create_dir_all(dir.path().join(DEFAULT_SURFACE)).unwrap();
        assert_eq!(
            paths(dir.path(), &[], config),
            [Ok(dir.path().join(DEFAULT_SURFACE))]
        );

        fs::create_dir_all(dir.path().join(".auditaur")).unwrap();
        fs::write(
            dir.path().join(config),
            r#"{ "app": "x", "coverage": { "commands": ["app/src"] } }"#,
        )
        .unwrap();
        assert_eq!(
            paths(dir.path(), &[], config),
            [Ok(dir.path().join("app/src"))]
        );

        let flag = PathBuf::from("cmds.txt");
        assert_eq!(
            paths(dir.path(), std::slice::from_ref(&flag), config),
            [Ok(flag)]
        );

        fs::write(
            dir.path().join(config),
            r#"{ "coverage": { "commands": "x" } }"#,
        )
        .unwrap();
        let unreadable = paths(dir.path(), &[], config);
        assert!(unreadable[0]
            .as_ref()
            .unwrap_err()
            .error
            .as_deref()
            .unwrap()
            .contains("array of paths"));
    }
}
