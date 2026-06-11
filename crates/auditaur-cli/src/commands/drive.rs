use std::{
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::Value;

use crate::{
    commands::read,
    discovery::{self, DiscoveredApp, DiscoveryStatus},
    output::table_cell,
};

const DEFAULT_CDP_PORTS: &[u16] = &[9222, 9223, 9224, 9225, 9226, 9227, 9228, 9229, 9230];
const CDP_HOST: &str = "127.0.0.1";

pub fn run(app: Option<String>, cdp_port: Option<u16>, json: bool) -> Result<()> {
    let target = resolve_target(app.as_deref())?;
    let attach = DriveAttachInfo::discover(target, cdp_port)?;
    read::print_json_or_table(json, &attach, || print_attach_info(&attach))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DriveAttachInfo {
    status: String,
    service_name: String,
    service_version: Option<String>,
    app_identifier: Option<String>,
    pid: u32,
    instance_id: String,
    session_id: String,
    started_at: String,
    last_heartbeat_at: String,
    db_path: String,
    discovery_path: String,
    cdp: CdpAttachInfo,
    future_actions: Vec<DriverActionSpec>,
    required_action_telemetry: Vec<&'static str>,
    note: String,
}

impl DriveAttachInfo {
    fn discover(app: DiscoveredApp, cdp_port: Option<u16>) -> Result<Self> {
        let cdp = CdpAttachInfo::discover(cdp_port)?;
        Ok(Self {
            status: match app.status {
                DiscoveryStatus::Active => "active".to_string(),
                DiscoveryStatus::Stale => "stale".to_string(),
            },
            service_name: app.service_name,
            service_version: app.service_version,
            app_identifier: app.app_identifier,
            pid: app.pid,
            instance_id: app.instance_id,
            session_id: app.session_id,
            started_at: app.started_at,
            last_heartbeat_at: app.last_heartbeat_at,
            db_path: app.database_path,
            discovery_path: app.discovery_path,
            cdp,
            future_actions: future_actions(),
            required_action_telemetry: required_action_telemetry(),
            note: "Drive is an optional diagnostic attach layer; it does not mutate Auditaur's read-only telemetry store.".to_string(),
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CdpAttachInfo {
    status: String,
    endpoint: Option<String>,
    port: Option<u16>,
    product: Option<String>,
    browser_protocol_version: Option<String>,
    reason: Option<String>,
    launch_hint: String,
}

impl CdpAttachInfo {
    fn discover(cdp_port: Option<u16>) -> Result<Self> {
        let ports: Vec<u16> = cdp_port
            .map(|port| vec![port])
            .unwrap_or_else(|| DEFAULT_CDP_PORTS.to_vec());

        for port in ports {
            match probe_cdp_version(port)? {
                Some(version) => {
                    return Ok(Self {
                        status: "available".to_string(),
                        endpoint: Some(format!("http://{CDP_HOST}:{port}")),
                        port: Some(port),
                        product: json_string(&version, "Browser")
                            .or_else(|| json_string(&version, "Product")),
                        browser_protocol_version: json_string(&version, "Protocol-Version"),
                        reason: None,
                        launch_hint: launch_hint(cdp_port),
                    })
                }
                None => continue,
            }
        }

        Ok(Self {
            status: "unavailable".to_string(),
            endpoint: None,
            port: cdp_port,
            product: None,
            browser_protocol_version: None,
            reason: Some("No Chrome DevTools Protocol /json/version endpoint responded on the probed localhost port(s).".to_string()),
            launch_hint: launch_hint(cdp_port),
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DriverActionSpec {
    name: &'static str,
    selector_required: bool,
    description: &'static str,
}

fn resolve_target(app: Option<&str>) -> Result<DiscoveredApp> {
    let mut candidates: Vec<_> = discovery::list_apps()?
        .into_iter()
        .filter(|candidate| candidate.database_readable && candidate.schema_valid)
        .filter(|candidate| app.is_none_or(|needle| app_matches(candidate, needle)))
        .collect();

    candidates.sort_by(|left, right| {
        let left_active = left.status == DiscoveryStatus::Active;
        let right_active = right.status == DiscoveryStatus::Active;
        right_active
            .cmp(&left_active)
            .then_with(|| right.last_heartbeat_at.cmp(&left.last_heartbeat_at))
    });

    let active_count = candidates
        .iter()
        .filter(|candidate| candidate.status == DiscoveryStatus::Active)
        .count();

    match candidates.as_slice() {
        [] => Err(anyhow!(
            "No discoverable Auditaur app matched {}. Run `auditaur apps` to inspect available sessions.",
            app.map(|value| format!("`{value}`")).unwrap_or_else(|| "the active app".to_string())
        )),
        [candidate] => Ok(candidate.clone()),
        _ if active_count == 1 => Ok(candidates
            .into_iter()
            .find(|candidate| candidate.status == DiscoveryStatus::Active)
            .expect("counted one active candidate")),
        _ => Err(anyhow!(
            "Multiple Auditaur apps matched. Run `auditaur apps` and pass a more specific `--app <name>`."
        )),
    }
}

fn app_matches(candidate: &DiscoveredApp, needle: &str) -> bool {
    let needle = needle.to_ascii_lowercase();
    candidate
        .service_name
        .to_ascii_lowercase()
        .contains(&needle)
        || candidate
            .app_identifier
            .as_deref()
            .is_some_and(|identifier| identifier.to_ascii_lowercase().contains(&needle))
        || candidate.session_id.to_ascii_lowercase().contains(&needle)
}

fn probe_cdp_version(port: u16) -> Result<Option<Value>> {
    let mut addrs = (CDP_HOST, port)
        .to_socket_addrs()
        .with_context(|| format!("could not resolve {CDP_HOST}:{port}"))?;
    let Some(addr) = addrs.next() else {
        return Ok(None);
    };
    let timeout = Duration::from_millis(150);
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, timeout) else {
        return Ok(None);
    };
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(
        format!(
            "GET /json/version HTTP/1.1\r\nHost: {CDP_HOST}:{port}\r\nConnection: close\r\n\r\n"
        )
        .as_bytes(),
    )?;

    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    if !response.starts_with("HTTP/1.1 200") && !response.starts_with("HTTP/1.0 200") {
        return Ok(None);
    }
    let Some((_, body)) = response.split_once("\r\n\r\n") else {
        return Ok(None);
    };
    Ok(serde_json::from_str(body).ok())
}

fn json_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn launch_hint(cdp_port: Option<u16>) -> String {
    let port = cdp_port.unwrap_or(9222);
    format!(
        "Launch the Tauri/WebView app with remote debugging enabled, for example set WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=\"--remote-debugging-port={port}\" before starting the app, then rerun `auditaur drive --app <name>{}`.",
        if cdp_port.is_some() {
            format!(" --cdp-port {port}")
        } else {
            String::new()
        }
    )
}

fn future_actions() -> Vec<DriverActionSpec> {
    vec![
        DriverActionSpec {
            name: "click",
            selector_required: true,
            description: "activate an element by selector",
        },
        DriverActionSpec {
            name: "fill",
            selector_required: true,
            description: "set text in an editable element by selector",
        },
        DriverActionSpec {
            name: "press",
            selector_required: false,
            description: "send a keyboard key or chord",
        },
        DriverActionSpec {
            name: "wait",
            selector_required: false,
            description: "wait for a selector, text, or timeout",
        },
    ]
}

fn required_action_telemetry() -> Vec<&'static str> {
    vec![
        "auditaur.test_id",
        "auditaur.step_id",
        "auditaur.driver.action",
        "auditaur.driver.selector",
        "tauri.window.label",
        "trace_id",
        "span_id",
    ]
}

fn print_attach_info(info: &DriveAttachInfo) -> Result<()> {
    println!("Auditaur drive attach: {}", info.status);
    println!("Service: {}", table_cell(&info.service_name, 80));
    if let Some(identifier) = &info.app_identifier {
        println!("App identifier: {}", table_cell(identifier, 120));
    }
    println!("PID: {}", info.pid);
    println!("Session: {}", table_cell(&info.session_id, 120));
    println!("Database: {}", table_cell(&info.db_path, 180));
    match info.cdp.status.as_str() {
        "available" => println!(
            "CDP: available at {} ({})",
            info.cdp.endpoint.as_deref().unwrap_or("-"),
            info.cdp.product.as_deref().unwrap_or("unknown product")
        ),
        _ => {
            println!("CDP: unavailable");
            println!("{}", info.cdp.launch_hint);
        }
    }
    println!(
        "Future action telemetry: {}",
        info.required_action_telemetry.join(", ")
    );
    Ok(())
}
