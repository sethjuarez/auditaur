use std::{
    io::{ErrorKind, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use auditaur_core::{model::TauriWindowState, storage::TauriWindowQuery};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tungstenite::{
    client, client::IntoClientRequest, stream::MaybeTlsStream, Error as WsError, Message,
};

use crate::{
    commands::read,
    discovery::{self, DiscoveredApp, DiscoveryStatus},
    output::table_cell,
};

const DEFAULT_CDP_PORTS: &[u16] = &[9222, 9223, 9224, 9225, 9226, 9227, 9228, 9229, 9230];
const CDP_HOST: &str = "127.0.0.1";
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(100);
const CDP_READ_TIMEOUT: Duration = Duration::from_millis(500);

pub fn run(app: Option<String>, cdp_port: Option<u16>, json: bool) -> Result<()> {
    let target = resolve_target(app.as_deref())?;
    let attach = DriveAttachInfo::discover(target, cdp_port)?;
    read::print_json_or_table(json, &attach, || print_attach_info(&attach, false))
}

fn bounded_timeout(timeout: Duration) -> Duration {
    timeout.min(CDP_READ_TIMEOUT)
}

pub fn inspect(app: Option<String>, cdp_port: Option<u16>, json: bool) -> Result<()> {
    let target = resolve_target(app.as_deref())?;
    let attach = DriveAttachInfo::discover(target, cdp_port)?;
    read::print_json_or_table(json, &attach, || print_attach_info(&attach, true))
}

pub fn wait(app: Option<String>, cdp_port: Option<u16>, options: WaitOptions) -> Result<()> {
    if cdp_port.is_none() {
        return Err(anyhow!(
            "`auditaur drive wait` requires --cdp-port <port>. Run `auditaur drive inspect` first, then pass the WebView remote-debugging port explicitly."
        ));
    }
    let target = resolve_target(app.as_deref())?;
    let attach = DriveAttachInfo::discover(target, cdp_port)?;
    let cdp_target = select_cdp_target(&attach.cdp.targets, options.target_id.as_deref())?;
    let websocket_url = cdp_target
        .web_socket_debugger_url
        .as_deref()
        .ok_or_else(|| {
            anyhow!(
                "CDP target `{}` does not expose a WebSocket debugger URL.",
                cdp_target.id
            )
        })?;
    let wait_result = wait_for_selector(&attach, cdp_target, websocket_url, &options)?;
    let matched = wait_result.matched;
    read::print_json_or_table(options.json, &wait_result, || {
        print_wait_result(&wait_result)
    })?;
    if matched {
        Ok(())
    } else {
        Err(anyhow!(
            "Timed out after {}ms waiting for selector `{}`.",
            options.timeout_ms,
            options.selector
        ))
    }
}

#[derive(Debug)]
pub struct WaitOptions {
    pub selector: String,
    pub target_id: Option<String>,
    pub timeout_ms: u64,
    pub test_id: Option<String>,
    pub step_id: Option<String>,
    pub json: bool,
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
        let cdp = CdpAttachInfo::discover(cdp_port, &app)?;
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
            note: "Drive is an optional app-driver layer; it observes Auditaur discovery metadata and talks to a separate CDP endpoint instead of mutating Auditaur's telemetry store.".to_string(),
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
    target_binding_status: String,
    target_binding_note: String,
    target_discovery_error: Option<String>,
    targets: Vec<CdpTarget>,
}

impl CdpAttachInfo {
    fn discover(cdp_port: Option<u16>, app: &DiscoveredApp) -> Result<Self> {
        let ports: Vec<u16> = cdp_port
            .map(|port| vec![port])
            .unwrap_or_else(|| DEFAULT_CDP_PORTS.to_vec());

        for port in ports {
            let Some(version) = get_cdp_json(port, "/json/version")? else {
                continue;
            };
            let (targets, target_discovery_error) = match list_cdp_targets(port) {
                Ok(targets) => (bind_targets_to_windows(targets, app), None),
                Err(error) => (Vec::new(), Some(error.to_string())),
            };
            let (target_binding_status, target_binding_note) = target_binding_summary(&targets);
            return Ok(Self {
                status: "available".to_string(),
                endpoint: Some(format!("http://{CDP_HOST}:{port}")),
                port: Some(port),
                product: json_string(&version, "Browser")
                    .or_else(|| json_string(&version, "Product")),
                browser_protocol_version: json_string(&version, "Protocol-Version"),
                reason: None,
                launch_hint: launch_hint(cdp_port),
                target_binding_status,
                target_binding_note,
                target_discovery_error,
                targets,
            });
        }

        Ok(Self {
            status: "unavailable".to_string(),
            endpoint: None,
            port: cdp_port,
            product: None,
            browser_protocol_version: None,
            reason: Some("No Chrome DevTools Protocol /json/version endpoint responded on the probed localhost port(s).".to_string()),
            launch_hint: launch_hint(cdp_port),
            target_binding_status: "unavailable".to_string(),
            target_binding_note: "No CDP endpoint was available to bind to the observed app.".to_string(),
            target_discovery_error: None,
            targets: Vec::new(),
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CdpTarget {
    id: String,
    #[serde(rename = "type")]
    target_type: Option<String>,
    title: Option<String>,
    url: Option<String>,
    #[serde(rename = "webSocketDebuggerUrl")]
    web_socket_debugger_url: Option<String>,
    #[serde(default)]
    binding_status: String,
    #[serde(default)]
    binding_reason: Option<String>,
    #[serde(default)]
    window_label: Option<String>,
    #[serde(default)]
    webview_label: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DriverActionSpec {
    name: &'static str,
    selector_required: bool,
    mutates_app: bool,
    description: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WaitResult {
    ok: bool,
    action: &'static str,
    selector: String,
    matched: bool,
    elapsed_ms: u128,
    timeout_ms: u64,
    service_name: String,
    pid: u32,
    session_id: String,
    target_id: String,
    target_title: Option<String>,
    target_url: Option<String>,
    window_label: Option<String>,
    test_id: Option<String>,
    step_id: Option<String>,
    telemetry_attributes: Value,
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

fn list_cdp_targets(port: u16) -> Result<Vec<CdpTarget>> {
    let Some(value) = get_cdp_json(port, "/json/list")? else {
        return Ok(Vec::new());
    };
    serde_json::from_value(value).context("CDP /json/list did not return a target array")
}

fn bind_targets_to_windows(mut targets: Vec<CdpTarget>, app: &DiscoveredApp) -> Vec<CdpTarget> {
    let windows = match read::open_validated_store(std::path::Path::new(&app.database_path))
        .and_then(|store| {
            Ok(store.list_tauri_windows(&TauriWindowQuery {
                session_id: Some(app.session_id.clone()),
                latest_only: true,
                limit: Some(50),
            })?)
        }) {
        Ok(windows) => windows,
        Err(error) => {
            for target in &mut targets {
                target.binding_status = "unverified".to_string();
                target.binding_reason = Some(format!(
                    "Could not read observed Tauri window telemetry for binding: {error}"
                ));
            }
            return targets;
        }
    };
    let driveable_target_count = targets
        .iter()
        .filter(|target| is_driveable_target(target))
        .count();
    let allow_probable_single_window = driveable_target_count == 1 && windows.len() == 1;
    for target in &mut targets {
        bind_target_to_windows(target, &windows, allow_probable_single_window);
    }
    targets
}

fn bind_target_to_windows(
    target: &mut CdpTarget,
    windows: &[TauriWindowState],
    allow_probable_single_window: bool,
) {
    if let Some(window) = windows.iter().find(|window| title_matches(target, window)) {
        target.binding_status = "matched_window_title".to_string();
        target.binding_reason = Some(format!(
            "CDP target title matched observed Tauri window `{}` title.",
            window.window_label
        ));
        target.window_label = Some(window.window_label.clone());
        target.webview_label = window.webview_label.clone();
        return;
    }

    if let Some(window) = windows.iter().find(|window| url_matches(target, window)) {
        target.binding_status = "matched_window_url".to_string();
        target.binding_reason = Some(format!(
            "CDP target URL matched observed Tauri window `{}` URL.",
            window.window_label
        ));
        target.window_label = Some(window.window_label.clone());
        target.webview_label = window.webview_label.clone();
        return;
    }

    if allow_probable_single_window {
        target.binding_status = "probable_single_window".to_string();
        target.binding_reason = Some(format!(
            "Only one observed Tauri window (`{}`) is available for this session; treat as probable, not proven.",
            windows[0].window_label
        ));
        target.window_label = Some(windows[0].window_label.clone());
        target.webview_label = windows[0].webview_label.clone();
        return;
    }

    target.binding_status = "unverified".to_string();
    target.binding_reason =
        Some("No observed Tauri window title or URL matched this CDP target.".to_string());
}

fn title_matches(target: &CdpTarget, window: &TauriWindowState) -> bool {
    normalized(target.title.as_deref())
        .zip(normalized(window.title.as_deref()))
        .is_some_and(|(target_title, window_title)| target_title == window_title)
}

fn url_matches(target: &CdpTarget, window: &TauriWindowState) -> bool {
    normalized(target.url.as_deref())
        .zip(normalized(window.url.as_deref()))
        .is_some_and(|(target_url, window_url)| target_url == window_url)
}

fn normalized(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    (!value.is_empty()).then(|| value.to_ascii_lowercase())
}

fn target_binding_summary(targets: &[CdpTarget]) -> (String, String) {
    let matched = targets
        .iter()
        .filter(|target| target.binding_status.starts_with("matched_"))
        .count();
    let probable = targets
        .iter()
        .filter(|target| target.binding_status == "probable_single_window")
        .count();
    if matched > 0 {
        (
            "matched".to_string(),
            format!("{matched} CDP target(s) matched observed Auditaur window telemetry."),
        )
    } else if probable > 0 {
        (
            "probable".to_string(),
            format!("{probable} CDP target(s) were associated by single-window session context."),
        )
    } else if targets.is_empty() {
        (
            "unavailable".to_string(),
            "No CDP targets were available to bind.".to_string(),
        )
    } else {
        (
            "unverified".to_string(),
            "CDP targets were discovered, but none matched observed Auditaur window title or URL telemetry.".to_string(),
        )
    }
}

fn get_cdp_json(port: u16, path: &str) -> Result<Option<Value>> {
    let response = http_get(port, path)?;
    let Some(response) = response else {
        return Ok(None);
    };
    if !response.starts_with("HTTP/1.1 200") && !response.starts_with("HTTP/1.0 200") {
        return Ok(None);
    }
    let Some((_, body)) = response.split_once("\r\n\r\n") else {
        return Ok(None);
    };
    Ok(serde_json::from_str(body).ok())
}

fn http_get(port: u16, path: &str) -> Result<Option<String>> {
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
    if let Err(error) = stream.write_all(
        format!("GET {path} HTTP/1.1\r\nHost: {CDP_HOST}:{port}\r\nConnection: close\r\n\r\n")
            .as_bytes(),
    ) {
        if is_probe_io_miss(&error) {
            return Ok(None);
        }
        return Err(error.into());
    }

    let mut response = String::new();
    if let Err(error) = stream.read_to_string(&mut response) {
        if is_probe_io_miss(&error) {
            return Ok(None);
        }
        return Err(error.into());
    }
    Ok(Some(response))
}

fn is_probe_io_miss(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::WouldBlock
            | ErrorKind::TimedOut
            | ErrorKind::ConnectionAborted
            | ErrorKind::ConnectionReset
            | ErrorKind::UnexpectedEof
    )
}

fn select_cdp_target<'a>(
    targets: &'a [CdpTarget],
    target_id: Option<&str>,
) -> Result<&'a CdpTarget> {
    if let Some(target_id) = target_id {
        return targets
            .iter()
            .find(|target| target.id == target_id)
            .ok_or_else(|| {
                anyhow!("No CDP target matched `{target_id}`. Run `auditaur drive inspect`.")
            });
    }

    let driveable: Vec<_> = targets
        .iter()
        .filter(|target| is_driveable_target(target))
        .collect();
    if driveable.len() > 1 {
        let bound: Vec<_> = driveable
            .iter()
            .copied()
            .filter(|target| is_bound_target(target))
            .collect();
        match bound.as_slice() {
            [target] => return Ok(target),
            [] => {}
            _ => {
                return Err(anyhow!(
                    "Multiple bound CDP targets found. Run `auditaur drive inspect` and pass --target <target-id>."
                ))
            }
        }
    }
    match driveable.as_slice() {
        [target] => Ok(target),
        [] => match targets
            .iter()
            .filter(|target| target.web_socket_debugger_url.is_some())
            .collect::<Vec<_>>()
            .as_slice()
        {
            [target] => Ok(target),
            [] => Err(anyhow!("No driveable CDP page target found. Run `auditaur drive inspect`.")),
            _ => Err(anyhow!(
                "Multiple driveable CDP targets found. Run `auditaur drive inspect` and pass --target <target-id>."
            )),
        },
        _ => Err(anyhow!(
            "Multiple driveable CDP targets found. Run `auditaur drive inspect` and pass --target <target-id>."
        )),
    }
}

fn is_bound_target(target: &CdpTarget) -> bool {
    target.binding_status.starts_with("matched_")
        || target.binding_status == "probable_single_window"
}

fn is_driveable_target(target: &CdpTarget) -> bool {
    target.web_socket_debugger_url.is_some()
        && target
            .target_type
            .as_deref()
            .map(|kind| matches!(kind, "page" | "webview"))
            .unwrap_or(true)
}

fn wait_for_selector(
    attach: &DriveAttachInfo,
    target: &CdpTarget,
    websocket_url: &str,
    options: &WaitOptions,
) -> Result<WaitResult> {
    let timeout = Duration::from_millis(options.timeout_ms);
    let started = Instant::now();
    let deadline = started + timeout;
    let mut socket = connect_cdp_websocket(websocket_url, timeout)
        .with_context(|| format!("failed to connect to {websocket_url}"))?;
    let mut next_id = 1_u64;

    send_cdp_command(&mut socket, next_id, "Runtime.enable", json!({}))?;
    if read_cdp_response(&mut socket, next_id, deadline)?.is_none() {
        return Ok(wait_result(
            attach,
            target,
            options,
            false,
            started.elapsed().as_millis(),
        ));
    }
    next_id += 1;

    let expression = selector_expression(&options.selector)?;
    while Instant::now() <= deadline {
        let command_id = next_id;
        next_id += 1;
        send_cdp_command(
            &mut socket,
            command_id,
            "Runtime.evaluate",
            json!({
                "expression": expression,
                "returnByValue": true,
                "awaitPromise": false,
            }),
        )?;
        let Some(response) = read_cdp_response(&mut socket, command_id, deadline)? else {
            return Ok(wait_result(
                attach,
                target,
                options,
                false,
                started.elapsed().as_millis(),
            ));
        };
        if response
            .get("result")
            .and_then(|result| result.get("exceptionDetails"))
            .is_some()
        {
            return Err(anyhow!("CDP Runtime.evaluate failed: {response}"));
        }
        if response
            .pointer("/result/result/value")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            let elapsed_ms = started.elapsed().as_millis();
            return Ok(wait_result(attach, target, options, true, elapsed_ms));
        }
        thread::sleep(WAIT_POLL_INTERVAL);
    }

    Ok(wait_result(
        attach,
        target,
        options,
        false,
        started.elapsed().as_millis(),
    ))
}

fn send_cdp_command(
    socket: &mut tungstenite::WebSocket<MaybeTlsStream<TcpStream>>,
    id: u64,
    method: &str,
    params: Value,
) -> Result<()> {
    let message = json!({
        "id": id,
        "method": method,
        "params": params,
    });
    socket.send(Message::Text(message.to_string().into()))?;
    Ok(())
}

fn connect_cdp_websocket(
    websocket_url: &str,
    timeout: Duration,
) -> Result<tungstenite::WebSocket<MaybeTlsStream<TcpStream>>> {
    let (host, port) = parse_ws_endpoint(websocket_url)?;
    let mut addrs = (host.as_str(), port)
        .to_socket_addrs()
        .with_context(|| format!("could not resolve {host}:{port}"))?;
    let Some(addr) = addrs.next() else {
        return Err(anyhow!("could not resolve {host}:{port}"));
    };
    let handshake_timeout = bounded_timeout(timeout);
    let stream = TcpStream::connect_timeout(&addr, handshake_timeout)?;
    stream.set_read_timeout(Some(handshake_timeout))?;
    stream.set_write_timeout(Some(handshake_timeout))?;
    let request = websocket_url.into_client_request()?;
    let (socket, _) = client(request, MaybeTlsStream::Plain(stream))?;
    Ok(socket)
}

fn parse_ws_endpoint(websocket_url: &str) -> Result<(String, u16)> {
    let rest = websocket_url
        .strip_prefix("ws://")
        .ok_or_else(|| anyhow!("Only ws:// CDP endpoints are supported: {websocket_url}"))?;
    let host_port = rest.split('/').next().unwrap_or(rest);
    let Some((host, port)) = host_port.rsplit_once(':') else {
        return Ok((host_port.to_string(), 80));
    };
    let port = port
        .parse()
        .with_context(|| format!("Invalid CDP WebSocket port in `{websocket_url}`"))?;
    Ok((host.to_string(), port))
}

fn read_cdp_response(
    socket: &mut tungstenite::WebSocket<MaybeTlsStream<TcpStream>>,
    id: u64,
    deadline: Instant,
) -> Result<Option<Value>> {
    loop {
        if Instant::now() > deadline {
            return Ok(None);
        }
        match socket.read() {
            Ok(message) => {
                if !message.is_text() {
                    continue;
                }
                let value: Value = serde_json::from_str(&message.into_text()?.to_string())?;
                if value.get("id").and_then(Value::as_u64) == Some(id) {
                    return Ok(Some(value));
                }
            }
            Err(WsError::Io(error))
                if error.kind() == ErrorKind::WouldBlock || error.kind() == ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn selector_expression(selector: &str) -> Result<String> {
    let selector_json = serde_json::to_string(selector)?;
    Ok(format!("Boolean(document.querySelector({selector_json}))"))
}

fn wait_result(
    attach: &DriveAttachInfo,
    target: &CdpTarget,
    options: &WaitOptions,
    matched: bool,
    elapsed_ms: u128,
) -> WaitResult {
    WaitResult {
        ok: matched,
        action: "wait",
        selector: options.selector.clone(),
        matched,
        elapsed_ms,
        timeout_ms: options.timeout_ms,
        service_name: attach.service_name.clone(),
        pid: attach.pid,
        session_id: attach.session_id.clone(),
        target_id: target.id.clone(),
        target_title: target.title.clone(),
        target_url: target.url.clone(),
        window_label: target.window_label.clone(),
        test_id: options.test_id.clone(),
        step_id: options.step_id.clone(),
        telemetry_attributes: json!({
            "auditaur.test_id": options.test_id,
            "auditaur.step_id": options.step_id,
            "auditaur.driver.action": "wait",
            "auditaur.driver.selector": options.selector,
            "auditaur.driver.target_id": target.id,
            "tauri.window.label": target.window_label,
            "trace_id": null,
            "span_id": null,
        }),
    }
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
            name: "wait",
            selector_required: true,
            mutates_app: false,
            description: "wait for a selector to appear through CDP Runtime.evaluate",
        },
        DriverActionSpec {
            name: "click",
            selector_required: true,
            mutates_app: true,
            description: "activate an element by selector",
        },
        DriverActionSpec {
            name: "fill",
            selector_required: true,
            mutates_app: true,
            description: "set text in an editable element by selector",
        },
        DriverActionSpec {
            name: "press",
            selector_required: false,
            mutates_app: true,
            description: "send a keyboard key or chord",
        },
    ]
}

fn required_action_telemetry() -> Vec<&'static str> {
    vec![
        "auditaur.test_id",
        "auditaur.step_id",
        "auditaur.driver.action",
        "auditaur.driver.selector",
        "auditaur.driver.target_id",
        "tauri.window.label",
        "trace_id",
        "span_id",
    ]
}

fn print_attach_info(info: &DriveAttachInfo, show_targets: bool) -> Result<()> {
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
            "CDP: available at {} ({}, {} target(s))",
            info.cdp.endpoint.as_deref().unwrap_or("-"),
            info.cdp.product.as_deref().unwrap_or("unknown product"),
            info.cdp.targets.len()
        ),
        _ => {
            println!("CDP: unavailable");
            println!("{}", info.cdp.launch_hint);
        }
    }
    if let Some(error) = &info.cdp.target_discovery_error {
        println!("Target discovery: {}", table_cell(error, 180));
    }
    println!(
        "Target binding: {} - {}",
        table_cell(&info.cdp.target_binding_status, 40),
        table_cell(&info.cdp.target_binding_note, 180)
    );
    if show_targets {
        print_targets(&info.cdp.targets);
    }
    println!(
        "Future action telemetry: {}",
        info.required_action_telemetry.join(", ")
    );
    Ok(())
}

fn print_targets(targets: &[CdpTarget]) {
    println!("TARGET\tTYPE\tTITLE\tURL\tWINDOW\tBINDING\tWEBSOCKET");
    for target in targets {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            table_cell(&target.id, 80),
            table_cell(target.target_type.as_deref().unwrap_or("-"), 24),
            table_cell(target.title.as_deref().unwrap_or("-"), 80),
            table_cell(target.url.as_deref().unwrap_or("-"), 120),
            table_cell(target.window_label.as_deref().unwrap_or("-"), 80),
            table_cell(&target.binding_status, 40),
            if target.web_socket_debugger_url.is_some() {
                "yes"
            } else {
                "no"
            }
        );
    }
}

fn print_wait_result(result: &WaitResult) -> Result<()> {
    println!(
        "wait {} selector {} in {}ms",
        if result.matched {
            "matched"
        } else {
            "timed out"
        },
        table_cell(&result.selector, 120),
        result.elapsed_ms
    );
    println!(
        "Target: {} {}",
        table_cell(&result.target_id, 80),
        table_cell(result.target_title.as_deref().unwrap_or("-"), 80)
    );
    println!("Session: {}", table_cell(&result.session_id, 120));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_ws_endpoint, selector_expression};

    #[test]
    fn selector_expression_escapes_css_selector_as_javascript_string() {
        assert_eq!(
            selector_expression(r#"[data-testid="save"]"#).unwrap(),
            r#"Boolean(document.querySelector("[data-testid=\"save\"]"))"#
        );
    }

    #[test]
    fn parses_ws_endpoint_host_and_port() {
        assert_eq!(
            parse_ws_endpoint("ws://127.0.0.1:9222/devtools/page/1").unwrap(),
            ("127.0.0.1".to_string(), 9222)
        );
    }
}
