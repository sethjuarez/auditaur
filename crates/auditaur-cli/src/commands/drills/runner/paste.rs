//! The clipboard a `paste` step hands the drive bridge's `paste` action.

use std::fs;
use std::io::Read;
use std::path::Path;

use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Map, Value};

use super::dialogs::unexpanded;
use crate::commands::drill::expand_drill_env;
use crate::commands::drills::model::PasteStep;

/// Pasted files travel through the bridge base64-encoded, so their total size is capped.
pub const MAX_PASTE_BYTES: u64 = 5 * 1024 * 1024;

/// The bridge payload: `{ text?, html?, files?: [{ name, type, base64 }] }`. File paths expand
/// `${NAME}` from `env` and resolve against the current directory (the workspace).
pub fn payload(paste: &PasteStep, env: &[(String, String)]) -> Result<String, String> {
    let mut payload = Map::new();
    if let Some(text) = &paste.text {
        payload.insert("text".into(), json!(text));
    }
    if let Some(html) = &paste.html {
        payload.insert("html".into(), json!(html));
    }
    let mut total = 0u64;
    let mut files = Vec::new();
    for file in &paste.files {
        let (path, _) = expand_drill_env(file, env);
        if let Some(name) = unexpanded(&path) {
            return Err(format!(
                "`{file}` uses ${{{name}}}, which is not in the drill env (suite `env` or a setup hook's env file)"
            ));
        }
        let mut bytes = Vec::new();
        fs::File::open(&path)
            .and_then(|file| {
                file.take(MAX_PASTE_BYTES - total + 1)
                    .read_to_end(&mut bytes)
            })
            .map_err(|error| format!("could not read `{path}`: {error}"))?;
        total += bytes.len() as u64;
        if total > MAX_PASTE_BYTES {
            return Err(format!(
                "pasted files total more than {} MiB; paste smaller files",
                MAX_PASTE_BYTES / 1024 / 1024
            ));
        }
        let name = Path::new(&path)
            .file_name()
            .map_or_else(|| path.clone(), |name| name.to_string_lossy().into_owned());
        files.push(json!({
            "name": name,
            "type": mime_type(&name),
            "base64": STANDARD.encode(bytes),
        }));
    }
    if !files.is_empty() {
        payload.insert("files".into(), Value::Array(files));
    }
    Ok(Value::Object(payload).to_string())
}

/// The `File.type` a browser gives a file with this name; empty when it doesn't know, as there.
fn mime_type(name: &str) -> &'static str {
    let extension = Path::new(name)
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "csv" => "text/csv",
        "json" => "application/json",
        "xml" => "text/xml",
        "zip" => "application/zip",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "mp4" => "video/mp4",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(files: Vec<String>) -> PasteStep {
        PasteStep {
            text: Some("hi".into()),
            html: None,
            files,
            target: None,
        }
    }

    #[test]
    fn files_expand_from_the_env_and_travel_base64_with_a_mime_type() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Logo.PNG"), b"png!").unwrap();
        let env = vec![("FIXTURES".to_string(), dir.path().display().to_string())];

        let payload = payload(&step(vec!["${FIXTURES}/Logo.PNG".into()]), &env).unwrap();

        let payload: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(
            payload,
            json!({
                "text": "hi",
                "files": [{ "name": "Logo.PNG", "type": "image/png", "base64": "cG5nIQ==" }],
            })
        );
    }

    #[test]
    fn an_unknown_env_name_or_missing_file_fails_the_step() {
        let error = payload(&step(vec!["${NOPE}/a.txt".into()]), &[]).unwrap_err();
        assert!(error.contains("${NOPE}"), "{error}");

        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.txt").display().to_string();
        let error = payload(&step(vec![missing.clone()]), &[]).unwrap_err();
        assert!(
            error.starts_with(&format!("could not read `{missing}`")),
            "{error}"
        );
    }

    #[test]
    fn files_are_capped() {
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("big.bin");
        fs::write(
            &big,
            vec![0u8; usize::try_from(MAX_PASTE_BYTES).unwrap() + 1],
        )
        .unwrap();

        let error = payload(&step(vec![big.display().to_string()]), &[]).unwrap_err();

        assert!(error.contains("more than 5 MiB"), "{error}");
    }

    #[test]
    fn unknown_extensions_have_no_type() {
        assert_eq!(mime_type("notes"), "");
        assert_eq!(mime_type("a.weird"), "");
        assert_eq!(mime_type("Slides.PPTX"), mime_type("deck.pptx"));
    }
}
