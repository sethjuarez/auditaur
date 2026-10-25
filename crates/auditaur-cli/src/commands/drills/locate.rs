//! `auditaur drill locate`: suggest robust drill locators for elements in a running app.
//!
//! An agent writing a drill usually knows a CSS selector (from source or an aria
//! snapshot); this asks the in-app bridge for the role/label/test-id locator
//! Playwright-style tests prefer, so the drill survives markup changes.

use std::{path::PathBuf, time::Duration};

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

use super::{model, runner::real::bridge_selector};
use crate::commands::{
    drive::{BridgeSession, DriveAppSelector},
    workflow,
};

const BRIDGE_TIMEOUT: Duration = Duration::from_secs(10);

pub struct LocateOptions {
    pub target: String,
    pub app: Option<String>,
    pub config: PathBuf,
    pub json: bool,
}

pub fn locate(options: LocateOptions) -> Result<()> {
    let selector = target_selector(&options.target)?;
    let session = connect(&options)?;
    let reply = session.request(
        "locate",
        Some(selector),
        None,
        Vec::new(),
        false,
        None,
        None,
        BRIDGE_TIMEOUT,
    )?;
    if let Some(error) = reply.error.filter(|_| !reply.ok) {
        if error.contains("unsupported drive bridge action") {
            bail!("the app's @auditaur/api is too old for `locate`; update it and rebuild the app ({error})");
        }
        bail!("locate failed: {error}");
    }
    let result = parse_reply(&reply.payload)?;
    if options.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        print!("{}", render(&result));
    }
    Ok(())
}

/// A drill locator (JSON object) or a CSS selector, as a bridge selector string.
pub fn target_selector(target: &str) -> Result<String> {
    let trimmed = target.trim();
    let value = if trimmed.starts_with('{') {
        serde_json::from_str(trimmed).map_err(|error| anyhow!("invalid locator JSON: {error}"))?
    } else {
        Value::String(target.to_string())
    };
    let locator = model::parse_locator_value(&value).map_err(|issues| {
        let messages: Vec<String> = issues.iter().map(ToString::to_string).collect();
        anyhow!("invalid locator: {}", messages.join("; "))
    })?;
    Ok(bridge_selector(&locator))
}

fn connect(options: &LocateOptions) -> Result<BridgeSession> {
    let app = match &options.app {
        Some(app) => app.clone(),
        None => workflow::load_launch_config(&options.config)?
            .map(|config| config.app)
            .ok_or_else(|| {
                anyhow!(
                    "pass `--app <name>` or create `{}` (run `auditaur init`)",
                    options.config.display()
                )
            })?,
    };
    let selector = DriveAppSelector {
        app: Some(app),
        session_id: None,
        instance_id: None,
        pid: None,
        latest: false,
        active: true,
    };
    BridgeSession::connect(&selector).map_err(|error| {
        anyhow!("{error:#}\nStart the app first; `locate` inspects a running app.")
    })
}

pub fn parse_reply(payload: &Value) -> Result<Value> {
    let count = payload
        .get("count")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("bridge reply had no `count`"))?;
    let elements = payload
        .get("elements")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("bridge reply had no `elements` array"))?;
    Ok(json!({ "count": count, "elements": elements }))
}

pub fn render(result: &Value) -> String {
    let count = result["count"].as_u64().unwrap_or(0);
    let elements = result["elements"].as_array().cloned().unwrap_or_default();
    if count == 0 {
        return "No elements match.\n".to_string();
    }
    let mut out = format!(
        "{count} element{} match{}",
        if count == 1 { "" } else { "s" },
        if count == 1 { "es" } else { "" }
    );
    if (elements.len() as u64) < count {
        out.push_str(&format!(" (showing {})", elements.len()));
    }
    out.push_str(":\n");
    for element in &elements {
        let tag = element["tag"].as_str().unwrap_or("?");
        let text = element["text"].as_str().unwrap_or("");
        let hidden = if element["visible"].as_bool() == Some(false) {
            " (hidden)"
        } else {
            ""
        };
        out.push_str(&format!("  {}\n", element["locator"]));
        if text.is_empty() {
            out.push_str(&format!("    <{tag}>{hidden}\n"));
        } else {
            out.push_str(&format!("    <{tag}> {text:?}{hidden}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_and_locator_targets_become_bridge_selectors() {
        assert_eq!(
            target_selector("li button").unwrap(),
            r#"locator:{"kind":"css","value":"li button"}"#
        );
        assert_eq!(
            target_selector(r#"{"role":"button","name":"Save"}"#).unwrap(),
            r#"locator:{"kind":"role","name":"Save","value":"button"}"#
        );
    }

    #[test]
    fn rejects_bad_targets() {
        assert!(target_selector("{nope")
            .unwrap_err()
            .to_string()
            .contains("invalid locator JSON"));
        assert!(target_selector("  ")
            .unwrap_err()
            .to_string()
            .contains("invalid locator"));
        assert!(target_selector(r#"{"bogus":1}"#)
            .unwrap_err()
            .to_string()
            .contains("invalid locator"));
    }

    #[test]
    fn parses_and_renders_matches() {
        let result = parse_reply(&json!({
            "ok": true,
            "count": 3,
            "elements": [
                { "locator": { "role": "button", "name": "Edit", "exact": true, "nth": 0 }, "tag": "button", "text": "Edit", "visible": true },
                { "locator": "#raw", "tag": "div", "text": "", "visible": false },
            ],
        }))
        .unwrap();
        assert_eq!(result["count"], 3);
        let text = render(&result);
        assert!(text.starts_with("3 elements match (showing 2):\n"));
        assert!(text.contains(r#"{"exact":true,"name":"Edit","nth":0,"role":"button"}"#));
        assert!(text.contains("<button> \"Edit\"\n"));
        assert!(text.contains("\"#raw\"\n    <div> (hidden)\n"));
        assert_eq!(
            render(&json!({ "count": 0, "elements": [] })),
            "No elements match.\n"
        );
        assert!(parse_reply(&json!({ "elements": [] })).is_err());
    }
}
