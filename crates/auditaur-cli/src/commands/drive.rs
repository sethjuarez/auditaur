use std::{
    fs,
    io::{ErrorKind, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use auditaur_core::{model::TauriWindowState, storage::TauriWindowQuery};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
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
const CDP_AUTO_PROBE_TIMEOUT: Duration = Duration::from_millis(500);
const CDP_EXPLICIT_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const CDP_READ_TIMEOUT: Duration = Duration::from_millis(500);
const FAILURE_SNAPSHOT_TEXT_LIMIT_CHARS: usize = 64 * 1024;

pub fn run(selector: DriveAppSelector, cdp_port: Option<u16>, json: bool) -> Result<()> {
    let target = resolve_target(&selector)?;
    let attach = DriveAttachInfo::discover(target, cdp_port)?;
    read::print_json_or_table(json, &attach, || print_attach_info(&attach, false))
}

fn bounded_timeout(timeout: Duration) -> Duration {
    timeout.min(CDP_READ_TIMEOUT)
}

pub fn inspect(selector: DriveAppSelector, cdp_port: Option<u16>, json: bool) -> Result<()> {
    let target = resolve_target(&selector)?;
    let attach = DriveAttachInfo::discover(target, cdp_port)?;
    read::print_json_or_table(json, &attach, || print_attach_info(&attach, true))
}

pub fn wait(selector: DriveAppSelector, cdp_port: Option<u16>, options: WaitOptions) -> Result<()> {
    if cdp_port.is_none() {
        return Err(anyhow!(
            "`auditaur drive wait` requires --cdp-port <port>. Run `auditaur drive inspect` first, then pass the WebView remote-debugging port explicitly."
        ));
    }
    let target = resolve_target(&selector)?;
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

pub fn exists(
    selector: DriveAppSelector,
    cdp_port: Option<u16>,
    options: SelectorActionOptions,
) -> Result<()> {
    let (attach, target, websocket_url) =
        resolve_drive_target(selector, cdp_port, options.target_id.as_deref(), "exists")?;
    let resolver = selector_resolver_js(&options.selector, options.visible_only)?;
    let expression = format!("(() => {{ {resolver} return Boolean(el); }})()");
    let value = evaluate_expression(&websocket_url, &expression, Duration::from_secs(5))?;
    let found = value.get("value").and_then(Value::as_bool).unwrap_or(false);
    let result = action_result(
        &attach,
        &target,
        "exists",
        Some(options.selector.clone()),
        options.visible_only,
        false,
        json!({ "exists": found }),
        &options.test_id,
        &options.step_id,
    );
    read::print_json_or_table(options.json, &result, || print_action_result(&result))?;
    if found {
        Ok(())
    } else {
        Err(anyhow!("Selector `{}` was not found.", options.selector))
    }
}

pub fn text(
    selector: DriveAppSelector,
    cdp_port: Option<u16>,
    options: SelectorActionOptions,
) -> Result<()> {
    let (attach, target, websocket_url) =
        resolve_drive_target(selector, cdp_port, options.target_id.as_deref(), "text")?;
    let resolver = selector_resolver_js(&options.selector, options.visible_only)?;
    let expression = format!(
        "(() => {{ {resolver} return el ? {{ found: true, visibleOnly, text: (el.innerText ?? el.textContent ?? '') }} : {{ found: false, visibleOnly, text: null }}; }})()"
    );
    let value = evaluate_expression(&websocket_url, &expression, Duration::from_secs(5))?;
    let payload = value
        .get("value")
        .cloned()
        .unwrap_or_else(|| json!({ "found": false, "text": null }));
    let found = payload
        .get("found")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let result = action_result(
        &attach,
        &target,
        "text",
        Some(options.selector.clone()),
        options.visible_only,
        false,
        payload,
        &options.test_id,
        &options.step_id,
    );
    read::print_json_or_table(options.json, &result, || print_action_result(&result))?;
    if found {
        Ok(())
    } else {
        Err(anyhow!("Selector `{}` was not found.", options.selector))
    }
}

pub fn click(
    selector: DriveAppSelector,
    cdp_port: Option<u16>,
    options: SelectorActionOptions,
) -> Result<()> {
    let resolver = selector_resolver_js(&options.selector, options.visible_only)?;
    let expression = format!(
        "(() => {{ {resolver} if (!el) return {{ ok: false, visibleOnly, error: 'selector not found' }}; el.scrollIntoView({{ block: 'center', inline: 'center' }}); el.click(); return {{ ok: true, visibleOnly }}; }})()"
    );
    run_dom_action(selector, cdp_port, &options, "click", expression)
}

pub fn fill(selector: DriveAppSelector, cdp_port: Option<u16>, options: FillOptions) -> Result<()> {
    let resolver = selector_resolver_js(&options.selector, options.visible_only)?;
    let value_json = serde_json::to_string(&options.value)?;
    let expression = format!(
        "(() => {{ {resolver} if (!el) return {{ ok: false, visibleOnly, error: 'selector not found' }}; const value = {value_json}; el.focus(); const valueDescriptor = el instanceof HTMLTextAreaElement ? Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value') : el instanceof HTMLInputElement ? Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value') : null; if (valueDescriptor?.set) {{ valueDescriptor.set.call(el, value); }} else if ('value' in el) {{ el.value = value; }} else if (el.isContentEditable) {{ el.textContent = value; }} else {{ return {{ ok: false, visibleOnly, error: 'selector is not editable' }}; }} const input = typeof InputEvent === 'function' ? new InputEvent('input', {{ bubbles: true, cancelable: true, inputType: 'insertText', data: value }}) : new Event('input', {{ bubbles: true, cancelable: true }}); el.dispatchEvent(input); el.dispatchEvent(new Event('change', {{ bubbles: true }})); return {{ ok: true, visibleOnly }}; }})()"
    );
    let selector_options = SelectorActionOptions {
        selector: options.selector,
        target_id: options.target_id,
        test_id: options.test_id,
        step_id: options.step_id,
        allow_unproven_target: options.allow_unproven_target,
        visible_only: options.visible_only,
        json: options.json,
    };
    run_dom_action(selector, cdp_port, &selector_options, "fill", expression)
}

pub fn type_text(
    selector: DriveAppSelector,
    cdp_port: Option<u16>,
    options: TypeOptions,
) -> Result<()> {
    let (attach, target, websocket_url) =
        resolve_drive_target(selector, cdp_port, options.target_id.as_deref(), "type")?;
    require_mutation_allowed(&target, "type", options.allow_unproven_target)?;
    let resolver = selector_resolver_js(&options.selector, options.visible_only)?;
    let expression = format!(
        "(() => {{ {resolver} if (!el) return {{ ok: false, visibleOnly, error: 'selector not found' }}; const editable = el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el.isContentEditable; if (!editable) return {{ ok: false, visibleOnly, error: 'selector is not editable' }}; el.focus(); return {{ ok: true, visibleOnly }}; }})()"
    );
    let value = evaluate_expression(&websocket_url, &expression, Duration::from_secs(5))?;
    let mut payload = value
        .get("value")
        .cloned()
        .unwrap_or_else(|| json!({ "ok": false, "error": "missing action result" }));
    let ok = payload.get("ok").and_then(Value::as_bool).unwrap_or(false);
    if ok {
        insert_text(&websocket_url, &options.value)?;
        if let Some(payload) = payload.as_object_mut() {
            payload.insert(
                "insertedCharacters".to_string(),
                json!(options.value.chars().count()),
            );
        }
    }
    let result = action_result(
        &attach,
        &target,
        "type",
        Some(options.selector.clone()),
        options.visible_only,
        true,
        payload.clone(),
        &options.test_id,
        &options.step_id,
    );
    read::print_json_or_table(options.json, &result, || print_action_result(&result))?;
    if ok {
        Ok(())
    } else {
        Err(anyhow!(
            "drive type failed: {}",
            payload
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
        ))
    }
}

pub fn press(
    selector: DriveAppSelector,
    cdp_port: Option<u16>,
    options: PressOptions,
) -> Result<()> {
    let key_json = serde_json::to_string(&options.key)?;
    let selector_json = match &options.selector {
        Some(selector) => serde_json::to_string(selector)?,
        None => "null".to_string(),
    };
    let expression = format!(
        "(() => {{ const selector = {selector_json}; const target = selector ? document.querySelector(selector) : (document.activeElement || document.body); if (!target) return {{ ok: false, error: 'selector not found' }}; target.focus?.(); const key = {key_json}; for (const type of ['keydown', 'keyup']) target.dispatchEvent(new KeyboardEvent(type, {{ key, bubbles: true, cancelable: true }})); return {{ ok: true }}; }})()"
    );
    let selector_options = SelectorActionOptions {
        selector: options
            .selector
            .unwrap_or_else(|| "<active-element>".to_string()),
        target_id: options.target_id,
        test_id: options.test_id,
        step_id: options.step_id,
        allow_unproven_target: options.allow_unproven_target,
        visible_only: false,
        json: options.json,
    };
    run_dom_action(selector, cdp_port, &selector_options, "press", expression)
}

pub fn screenshot(
    selector: DriveAppSelector,
    cdp_port: Option<u16>,
    options: ScreenshotOptions,
) -> Result<()> {
    let (attach, target, websocket_url) = resolve_drive_target(
        selector,
        cdp_port,
        options.target_id.as_deref(),
        "screenshot",
    )?;
    let bytes = capture_screenshot_bytes(&websocket_url)?;
    fs::write(&options.output, bytes)?;
    let mut payload = json!({ "output": options.output.to_string_lossy(), "format": "png" });
    if let Some(snapshot_output) = &options.snapshot_output {
        let (snapshot, snapshot_error) =
            match capture_page_snapshot(&websocket_url, options.selector.as_deref()) {
                Ok(snapshot) => (Some(snapshot), None),
                Err(error) => (None, Some(error.to_string())),
            };
        let manifest = json!({
            "schemaVersion": 1,
            "action": "screenshot",
            "serviceName": attach.service_name.clone(),
            "sessionId": attach.session_id.clone(),
            "pid": attach.pid,
            "targetId": target.id.clone(),
            "targetTitle": target.title.clone(),
            "targetUrl": target.url.clone(),
            "windowLabel": target.window_label.clone(),
            "targetBindingStatus": target.binding_status.clone(),
            "targetOwnershipStatus": target.ownership_status.clone(),
            "ownershipProven": target.ownership_proven,
            "selector": options.selector.clone(),
            "testId": options.test_id.clone(),
            "stepId": options.step_id.clone(),
            "artifacts": {
                "screenshot": options.output.to_string_lossy(),
                "snapshot": snapshot_output.to_string_lossy(),
            },
            "snapshotTextLimitCharacters": FAILURE_SNAPSHOT_TEXT_LIMIT_CHARS,
            "snapshot": snapshot,
            "snapshotError": snapshot_error,
        });
        fs::write(snapshot_output, serde_json::to_vec_pretty(&manifest)?)?;
        payload["snapshot"] = json!(snapshot_output.to_string_lossy());
        payload["snapshotTextLimitCharacters"] = json!(FAILURE_SNAPSHOT_TEXT_LIMIT_CHARS);
        if let Some(snapshot_error) = snapshot_error {
            payload["snapshotError"] = json!(snapshot_error);
        }
    }
    let result = action_result(
        &attach,
        &target,
        "screenshot",
        options.selector.clone(),
        false,
        false,
        payload,
        &options.test_id,
        &options.step_id,
    );
    read::print_json_or_table(options.json, &result, || print_action_result(&result))
}

#[derive(Debug)]
pub struct WaitOptions {
    pub selector: String,
    pub target_id: Option<String>,
    pub timeout_ms: u64,
    pub test_id: Option<String>,
    pub step_id: Option<String>,
    pub visible_only: bool,
    pub json: bool,
}

#[derive(Debug)]
pub struct SelectorActionOptions {
    pub selector: String,
    pub target_id: Option<String>,
    pub test_id: Option<String>,
    pub step_id: Option<String>,
    pub allow_unproven_target: bool,
    pub visible_only: bool,
    pub json: bool,
}

#[derive(Debug)]
pub struct FillOptions {
    pub selector: String,
    pub value: String,
    pub target_id: Option<String>,
    pub test_id: Option<String>,
    pub step_id: Option<String>,
    pub allow_unproven_target: bool,
    pub visible_only: bool,
    pub json: bool,
}

#[derive(Debug)]
pub struct TypeOptions {
    pub selector: String,
    pub value: String,
    pub target_id: Option<String>,
    pub test_id: Option<String>,
    pub step_id: Option<String>,
    pub allow_unproven_target: bool,
    pub visible_only: bool,
    pub json: bool,
}

#[derive(Debug)]
pub struct PressOptions {
    pub key: String,
    pub selector: Option<String>,
    pub target_id: Option<String>,
    pub test_id: Option<String>,
    pub step_id: Option<String>,
    pub allow_unproven_target: bool,
    pub json: bool,
}

#[derive(Debug)]
pub struct ScreenshotOptions {
    pub output: PathBuf,
    pub snapshot_output: Option<PathBuf>,
    pub selector: Option<String>,
    pub target_id: Option<String>,
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
    target_ownership_status: String,
    target_ownership_note: String,
    target_discovery_error: Option<String>,
    targets: Vec<CdpTarget>,
}

impl CdpAttachInfo {
    fn discover(cdp_port: Option<u16>, app: &DiscoveredApp) -> Result<Self> {
        let ports: Vec<u16> = cdp_port
            .map(|port| vec![port])
            .unwrap_or_else(|| DEFAULT_CDP_PORTS.to_vec());
        let explicit_port = cdp_port.is_some();
        let mut probe_errors = Vec::new();

        for port in ports {
            let probe_timeout = if explicit_port {
                CDP_EXPLICIT_PROBE_TIMEOUT
            } else {
                CDP_AUTO_PROBE_TIMEOUT
            };
            let version = match get_cdp_json(port, "/json/version", probe_timeout) {
                Ok(Some(version)) => version,
                Ok(None) => continue,
                Err(error) => {
                    let reason = format!(
                        "CDP probe failed for http://{CDP_HOST}:{port}/json/version: {error}"
                    );
                    if explicit_port {
                        return Ok(unavailable_cdp(cdp_port, reason));
                    }
                    probe_errors.push(reason);
                    continue;
                }
            };
            let (targets, target_discovery_error) = match list_cdp_targets(port) {
                Ok(targets) => (bind_targets_to_windows(targets, app), None),
                Err(error) => (Vec::new(), Some(error.to_string())),
            };
            let (target_binding_status, target_binding_note) = target_binding_summary(&targets);
            let (target_ownership_status, target_ownership_note) =
                target_ownership_summary(&targets);
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
                target_ownership_status,
                target_ownership_note,
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
            reason: Some(auto_probe_reason(&probe_errors)),
            launch_hint: launch_hint(cdp_port),
            target_binding_status: "unavailable".to_string(),
            target_binding_note: "No CDP endpoint was available to bind to the observed app."
                .to_string(),
            target_ownership_status: "unavailable".to_string(),
            target_ownership_note: "No CDP endpoint was available to prove ownership.".to_string(),
            target_discovery_error: None,
            targets: Vec::new(),
        })
    }
}

fn auto_probe_reason(probe_errors: &[String]) -> String {
    if probe_errors.is_empty() {
        return "No Chrome DevTools Protocol /json/version endpoint responded on the probed localhost port(s).".to_string();
    }
    let mut reason = format!(
        "No Chrome DevTools Protocol /json/version endpoint responded on the probed localhost port(s). Probe errors: {}",
        probe_errors
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join("; ")
    );
    if probe_errors.len() > 3 {
        reason.push_str(&format!("; plus {} more", probe_errors.len() - 3));
    }
    reason
}

fn unavailable_cdp(cdp_port: Option<u16>, reason: String) -> CdpAttachInfo {
    CdpAttachInfo {
        status: "unavailable".to_string(),
        endpoint: None,
        port: cdp_port,
        product: None,
        browser_protocol_version: None,
        reason: Some(reason),
        launch_hint: launch_hint(cdp_port),
        target_binding_status: "unavailable".to_string(),
        target_binding_note: "No CDP endpoint was available to bind to the observed app."
            .to_string(),
        target_ownership_status: "unavailable".to_string(),
        target_ownership_note: "No CDP endpoint was available to prove ownership.".to_string(),
        target_discovery_error: None,
        targets: Vec::new(),
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
    #[serde(default)]
    ownership_status: String,
    #[serde(default)]
    ownership_proof: Option<String>,
    #[serde(default)]
    ownership_proven: bool,
    #[serde(default)]
    ownership_guidance: Option<String>,
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
    visible_only: bool,
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
    target_binding_status: String,
    target_ownership_status: String,
    ownership_proven: bool,
    test_id: Option<String>,
    step_id: Option<String>,
    telemetry_attributes: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActionResult {
    ok: bool,
    action: String,
    selector: Option<String>,
    visible_only: bool,
    service_name: String,
    pid: u32,
    session_id: String,
    target_id: String,
    target_title: Option<String>,
    target_url: Option<String>,
    window_label: Option<String>,
    target_binding_status: String,
    target_ownership_status: String,
    ownership_proven: bool,
    mutates_app: bool,
    payload: Value,
    test_id: Option<String>,
    step_id: Option<String>,
    telemetry_attributes: Value,
}

#[derive(Debug, Clone)]
pub struct DriveAppSelector {
    pub app: Option<String>,
    pub session_id: Option<String>,
    pub instance_id: Option<String>,
    pub pid: Option<u32>,
    pub latest: bool,
    pub active: bool,
}

fn resolve_target(selector: &DriveAppSelector) -> Result<DiscoveredApp> {
    let mut candidates: Vec<_> = discovery::list_apps()?
        .into_iter()
        .filter(|candidate| candidate.database_readable && candidate.schema_valid)
        .filter(|candidate| {
            selector
                .app
                .as_deref()
                .is_none_or(|needle| app_matches(candidate, needle))
        })
        .filter(|candidate| {
            selector
                .session_id
                .as_deref()
                .is_none_or(|needle| candidate.session_id.contains(needle))
        })
        .filter(|candidate| {
            selector
                .instance_id
                .as_deref()
                .is_none_or(|needle| candidate.instance_id.contains(needle))
        })
        .filter(|candidate| selector.pid.is_none_or(|pid| candidate.pid == pid))
        .filter(|candidate| !selector.active || candidate.status == DiscoveryStatus::Active)
        .collect();

    candidates.sort_by(|left, right| {
        let left_active = left.status == DiscoveryStatus::Active;
        let right_active = right.status == DiscoveryStatus::Active;
        right_active
            .cmp(&left_active)
            .then_with(|| right.last_heartbeat_at.cmp(&left.last_heartbeat_at))
    });

    if selector.latest {
        return candidates.into_iter().next().ok_or_else(|| {
            anyhow!(
                "No discoverable Auditaur app matched {}.",
                selector_description(selector)
            )
        });
    }

    let active_count = candidates
        .iter()
        .filter(|candidate| candidate.status == DiscoveryStatus::Active)
        .count();

    match candidates.as_slice() {
        [] => Err(anyhow!(
            "No discoverable Auditaur app matched {}. Run `auditaur apps` to inspect available sessions.",
            selector_description(selector)
        )),
        [candidate] => Ok(candidate.clone()),
        _ if active_count == 1 => Ok(candidates
            .into_iter()
            .find(|candidate| candidate.status == DiscoveryStatus::Active)
            .expect("counted one active candidate")),
        _ => Err(anyhow!(
            "Multiple Auditaur apps matched {}. Pass --session-id, --instance-id, --pid, --latest, or --active.\n{}",
            selector_description(selector),
            format_candidate_hints(&candidates)
        )),
    }
}

fn selector_description(selector: &DriveAppSelector) -> String {
    let mut parts = Vec::new();
    if let Some(app) = &selector.app {
        parts.push(format!("app `{app}`"));
    }
    if let Some(session_id) = &selector.session_id {
        parts.push(format!("session id `{session_id}`"));
    }
    if let Some(instance_id) = &selector.instance_id {
        parts.push(format!("instance id `{instance_id}`"));
    }
    if let Some(pid) = selector.pid {
        parts.push(format!("pid `{pid}`"));
    }
    if selector.latest {
        parts.push("--latest".to_string());
    }
    if selector.active {
        parts.push("--active".to_string());
    }
    if parts.is_empty() {
        "the active app".to_string()
    } else {
        parts.join(", ")
    }
}

fn format_candidate_hints(candidates: &[DiscoveredApp]) -> String {
    let mut lines = vec!["Top matches:".to_string()];
    for candidate in candidates.iter().take(5) {
        lines.push(format!(
            "- service={} session={} instance={} pid={} status={:?} db={}",
            candidate.service_name,
            candidate.session_id,
            candidate.instance_id,
            candidate.pid,
            candidate.status,
            candidate.database_path
        ));
    }
    lines.join("\n")
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
    let Some(value) = get_cdp_json(port, "/json/list", CDP_EXPLICIT_PROBE_TIMEOUT)? else {
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
                mark_unverified_ownership(target);
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
        target.ownership_status = "matched_window_telemetry".to_string();
        target.ownership_proof = Some("window_title".to_string());
        target.ownership_proven = false;
        target.ownership_guidance = Some("CDP target title matched observed Auditaur window telemetry, but Auditaur has not independently proven that the CDP endpoint belongs to the observed process/session.".to_string());
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
        target.ownership_status = "matched_window_telemetry".to_string();
        target.ownership_proof = Some("window_url".to_string());
        target.ownership_proven = false;
        target.ownership_guidance = Some("CDP target URL matched observed Auditaur window telemetry, but Auditaur has not independently proven that the CDP endpoint belongs to the observed process/session.".to_string());
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
        target.ownership_status = "probable_unproven".to_string();
        target.ownership_proof = Some("single_window_single_target".to_string());
        target.ownership_proven = false;
        target.ownership_guidance = Some("Only one observed window and one driveable CDP target were present, so this is probable but unproven. Mutating actions require --allow-unproven-target.".to_string());
        return;
    }

    target.binding_status = "unverified".to_string();
    target.binding_reason =
        Some("No observed Tauri window title or URL matched this CDP target.".to_string());
    mark_unverified_ownership(target);
}

fn mark_unverified_ownership(target: &mut CdpTarget) {
    target.ownership_status = "unverified".to_string();
    target.ownership_proof = None;
    target.ownership_proven = false;
    target.ownership_guidance = Some(
        "Auditaur could not prove this CDP target belongs to the observed app session.".to_string(),
    );
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

fn target_ownership_summary(targets: &[CdpTarget]) -> (String, String) {
    if targets.is_empty() {
        return (
            "unavailable".to_string(),
            "No CDP targets were available to prove ownership.".to_string(),
        );
    }
    let matched = targets
        .iter()
        .filter(|target| target.ownership_status == "matched_window_telemetry")
        .count();
    let probable = targets
        .iter()
        .filter(|target| target.ownership_status == "probable_unproven")
        .count();
    if matched > 0 {
        (
            "matched_window_telemetry".to_string(),
            "One or more CDP targets matched observed window telemetry, but endpoint PID/session ownership is not independently proven yet.".to_string(),
        )
    } else if probable > 0 {
        (
            "probable_unproven".to_string(),
            "CDP target ownership is probable from single-window/single-target context, not proven. Mutating actions require --allow-unproven-target.".to_string(),
        )
    } else {
        (
            "unverified".to_string(),
            "No CDP target ownership evidence matched the observed app session.".to_string(),
        )
    }
}

fn get_cdp_json(port: u16, path: &str, timeout: Duration) -> Result<Option<Value>> {
    let response = http_get(port, path, timeout)?;
    let Some(response) = response else {
        return Ok(None);
    };
    if !response.starts_with("HTTP/1.1 200") && !response.starts_with("HTTP/1.0 200") {
        let status = response.lines().next().unwrap_or("<missing status line>");
        return Err(anyhow!("unexpected HTTP status `{status}`"));
    }
    let Some((_, body)) = response.split_once("\r\n\r\n") else {
        return Err(anyhow!(
            "HTTP response did not include a header/body separator"
        ));
    };
    Ok(Some(serde_json::from_str(body).with_context(|| {
        format!("CDP endpoint {path} returned invalid JSON")
    })?))
}

fn http_get(port: u16, path: &str, timeout: Duration) -> Result<Option<String>> {
    let mut addrs = (CDP_HOST, port)
        .to_socket_addrs()
        .with_context(|| format!("could not resolve {CDP_HOST}:{port}"))?;
    let Some(addr) = addrs.next() else {
        return Ok(None);
    };
    let mut stream = TcpStream::connect_timeout(&addr, timeout)
        .with_context(|| format!("could not connect to {CDP_HOST}:{port} within {timeout:?}"))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: {CDP_HOST}:{port}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .with_context(|| {
            format!("could not write HTTP probe request to {CDP_HOST}:{port}{path}")
        })?;

    let response = read_http_response(&mut stream).with_context(|| {
        format!("could not read HTTP probe response from {CDP_HOST}:{port}{path}")
    })?;
    Ok(Some(response))
}

fn read_http_response(stream: &mut TcpStream) -> Result<String> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
        if http_response_complete(&bytes)? {
            break;
        }
    }
    String::from_utf8(bytes).context("HTTP probe response was not valid UTF-8")
}

fn http_response_complete(bytes: &[u8]) -> Result<bool> {
    let Some(header_end) = find_header_end(bytes) else {
        return Ok(false);
    };
    let headers = std::str::from_utf8(&bytes[..header_end])
        .context("HTTP probe response headers were not valid UTF-8")?;
    let Some(content_length) = content_length(headers)? else {
        return Ok(false);
    };
    Ok(bytes.len() >= header_end + 4 + content_length)
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn content_length(headers: &str) -> Result<Option<usize>> {
    for line in headers.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            return Ok(Some(value.trim().parse()?));
        }
    }
    Ok(None)
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
                    "Multiple bound CDP targets found. Pass --target <target-id>. Candidates: {}",
                    target_candidates(&bound)
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
            [] => Err(anyhow!(
                "No driveable CDP page target found. Run `auditaur drive inspect`."
            )),
            _ => Err(anyhow!(
                "Multiple driveable CDP targets found. Pass --target <target-id>. Candidates: {}",
                target_candidates(
                    &targets
                        .iter()
                        .filter(|target| target.web_socket_debugger_url.is_some())
                        .collect::<Vec<_>>()
                )
            )),
        },
        _ => Err(anyhow!(
            "Multiple driveable CDP targets found. Pass --target <target-id>. Candidates: {}",
            target_candidates(&driveable)
        )),
    }
}

fn target_candidates(targets: &[&CdpTarget]) -> String {
    targets
        .iter()
        .take(5)
        .map(|target| {
            format!(
                "{} type={} binding={} ownership={}",
                target.id,
                target.target_type.as_deref().unwrap_or("<unknown>"),
                target.binding_status,
                target.ownership_status
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
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

fn resolve_drive_target(
    selector: DriveAppSelector,
    cdp_port: Option<u16>,
    target_id: Option<&str>,
    action: &str,
) -> Result<(DriveAttachInfo, CdpTarget, String)> {
    if cdp_port.is_none() {
        return Err(anyhow!(
            "`auditaur drive {action}` requires --cdp-port <port>. Run `auditaur drive inspect` first, then pass the WebView remote-debugging port explicitly."
        ));
    }
    let app = resolve_target(&selector)?;
    let attach = DriveAttachInfo::discover(app, cdp_port)?;
    let cdp_target = select_cdp_target(&attach.cdp.targets, target_id)?.clone();
    let websocket_url = cdp_target.web_socket_debugger_url.clone().ok_or_else(|| {
        anyhow!(
            "CDP target `{}` does not expose a WebSocket debugger URL.",
            cdp_target.id
        )
    })?;
    Ok((attach, cdp_target, websocket_url))
}

fn evaluate_expression(websocket_url: &str, expression: &str, timeout: Duration) -> Result<Value> {
    let mut socket = connect_cdp_websocket(websocket_url, timeout)?;
    let deadline = Instant::now() + timeout;
    let mut next_id = 1_u64;
    send_cdp_command(&mut socket, next_id, "Runtime.enable", json!({}))?;
    let _ = read_cdp_response(&mut socket, next_id, deadline)?;
    next_id += 1;
    send_cdp_command(
        &mut socket,
        next_id,
        "Runtime.evaluate",
        json!({
            "expression": expression,
            "returnByValue": true,
            "awaitPromise": false,
        }),
    )?;
    let response = read_cdp_response(&mut socket, next_id, deadline)?
        .ok_or_else(|| anyhow!("Timed out waiting for Runtime.evaluate response."))?;
    if response
        .get("result")
        .and_then(|result| result.get("exceptionDetails"))
        .is_some()
    {
        return Err(anyhow!("CDP Runtime.evaluate failed: {response}"));
    }
    response.pointer("/result/result").cloned().ok_or_else(|| {
        anyhow!("CDP Runtime.evaluate response did not include a result: {response}")
    })
}

fn insert_text(websocket_url: &str, text: &str) -> Result<()> {
    let mut socket = connect_cdp_websocket(websocket_url, Duration::from_secs(5))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    send_cdp_command(&mut socket, 1, "Input.insertText", json!({ "text": text }))?;
    let response = read_cdp_response(&mut socket, 1, deadline)?
        .ok_or_else(|| anyhow!("Timed out waiting for Input.insertText response."))?;
    if let Some(error) = response.get("error") {
        return Err(anyhow!("CDP Input.insertText failed: {error}"));
    }
    Ok(())
}

fn capture_screenshot_bytes(websocket_url: &str) -> Result<Vec<u8>> {
    let mut socket = connect_cdp_websocket(websocket_url, Duration::from_secs(10))?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut next_id = 1_u64;
    send_cdp_command(&mut socket, next_id, "Page.enable", json!({}))?;
    let _ = read_cdp_response(&mut socket, next_id, deadline)?;
    next_id += 1;
    send_cdp_command(
        &mut socket,
        next_id,
        "Page.captureScreenshot",
        json!({ "format": "png", "fromSurface": true }),
    )?;
    let response = read_cdp_response(&mut socket, next_id, deadline)?
        .ok_or_else(|| anyhow!("Timed out waiting for screenshot response."))?;
    let data = response
        .pointer("/result/data")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("CDP screenshot response did not include image data: {response}"))?;
    BASE64_STANDARD.decode(data).map_err(Into::into)
}

fn capture_page_snapshot(websocket_url: &str, selector: Option<&str>) -> Result<Value> {
    let selector_json = serde_json::to_string(&selector)?;
    let expression = format!(
        "(() => {{ const limit = {FAILURE_SNAPSHOT_TEXT_LIMIT_CHARS}; const selector = {selector_json}; const selected = selector ? document.querySelector(selector) : null; const clip = (value) => {{ const text = String(value ?? ''); return {{ value: text.slice(0, limit), truncated: text.length > limit, length: text.length }}; }}; return {{ title: document.title, url: location.href, bodyText: clip(document.body?.innerText ?? ''), html: clip(document.documentElement?.outerHTML ?? ''), selected: selected ? {{ selector, text: clip(selected.innerText ?? selected.textContent ?? ''), html: clip(selected.outerHTML ?? '') }} : (selector ? {{ selector, found: false }} : null) }}; }})()"
    );
    evaluate_expression(websocket_url, &expression, Duration::from_secs(5))?
        .get("value")
        .cloned()
        .ok_or_else(|| anyhow!("CDP page snapshot response did not include a value."))
}

fn run_dom_action(
    selector: DriveAppSelector,
    cdp_port: Option<u16>,
    options: &SelectorActionOptions,
    action: &str,
    expression: String,
) -> Result<()> {
    let (attach, target, websocket_url) =
        resolve_drive_target(selector, cdp_port, options.target_id.as_deref(), action)?;
    require_mutation_allowed(&target, action, options.allow_unproven_target)?;
    let value = evaluate_expression(&websocket_url, &expression, Duration::from_secs(5))?;
    let payload = value
        .get("value")
        .cloned()
        .unwrap_or_else(|| json!({ "ok": false, "error": "missing action result" }));
    let ok = payload.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let result = action_result(
        &attach,
        &target,
        action,
        Some(options.selector.clone()),
        options.visible_only,
        true,
        payload.clone(),
        &options.test_id,
        &options.step_id,
    );
    read::print_json_or_table(options.json, &result, || print_action_result(&result))?;
    if ok {
        Ok(())
    } else {
        Err(anyhow!(
            "drive {action} failed: {}",
            payload
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
        ))
    }
}

fn require_mutation_allowed(
    target: &CdpTarget,
    action: &str,
    allow_unproven_target: bool,
) -> Result<()> {
    if !target.ownership_proven && !allow_unproven_target {
        return Err(anyhow!(
            "`auditaur drive {action}` selected a CDP target (`{}`) whose endpoint ownership is not PID/session-proven (ownershipStatus={}, bindingStatus={}). Re-run `auditaur drive inspect` to review ownership guidance, pass --target <target-id> if needed, and add --allow-unproven-target to acknowledge the target is not PID/session-proven.",
            target.id,
            target.ownership_status,
            target.binding_status,
        ));
    }
    Ok(())
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

    let expression = selector_expression(&options.selector, options.visible_only)?;
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

fn selector_expression(selector: &str, visible_only: bool) -> Result<String> {
    let resolver = selector_resolver_js(selector, visible_only)?;
    Ok(format!("(() => {{ {resolver} return Boolean(el); }})()"))
}

fn selector_resolver_js(selector: &str, visible_only: bool) -> Result<String> {
    let selector_json = serde_json::to_string(selector)?;
    let visible_only_json = serde_json::to_string(&visible_only)?;
    Ok(format!(
        "const selector = {selector_json}; const visibleOnly = {visible_only_json}; const isVisible = (node) => {{ if (!(node instanceof Element)) return false; if (node.closest('[hidden],[inert],[aria-hidden=\"true\"]')) return false; const rects = node.getClientRects(); if (!rects.length) return false; for (let current = node; current; current = current.parentElement) {{ const style = getComputedStyle(current); if (style.display === 'none' || style.visibility === 'hidden' || style.visibility === 'collapse') return false; }} return true; }}; const matches = Array.from(document.querySelectorAll(selector)); const el = visibleOnly ? matches.find(isVisible) : matches[0];"
    ))
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
        visible_only: options.visible_only,
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
        target_binding_status: target.binding_status.clone(),
        target_ownership_status: target.ownership_status.clone(),
        ownership_proven: target.ownership_proven,
        test_id: options.test_id.clone(),
        step_id: options.step_id.clone(),
        telemetry_attributes: json!({
            "auditaur.test_id": options.test_id,
            "auditaur.step_id": options.step_id,
            "auditaur.driver.action": "wait",
            "auditaur.driver.selector": options.selector,
            "auditaur.driver.visible_only": options.visible_only,
            "auditaur.driver.target_id": target.id,
            "auditaur.driver.target_binding_status": target.binding_status,
            "auditaur.driver.target_ownership_status": target.ownership_status,
            "auditaur.driver.ownership_proven": target.ownership_proven,
            "tauri.window.label": target.window_label,
            "trace_id": null,
            "span_id": null,
        }),
    }
}

fn action_result(
    attach: &DriveAttachInfo,
    target: &CdpTarget,
    action: &str,
    selector: Option<String>,
    visible_only: bool,
    mutates_app: bool,
    payload: Value,
    test_id: &Option<String>,
    step_id: &Option<String>,
) -> ActionResult {
    ActionResult {
        ok: payload
            .get("ok")
            .and_then(Value::as_bool)
            .unwrap_or_else(|| {
                payload
                    .get("exists")
                    .and_then(Value::as_bool)
                    .unwrap_or_else(|| {
                        payload
                            .get("found")
                            .and_then(Value::as_bool)
                            .unwrap_or(true)
                    })
            }),
        action: action.to_string(),
        selector: selector.clone(),
        visible_only,
        service_name: attach.service_name.clone(),
        pid: attach.pid,
        session_id: attach.session_id.clone(),
        target_id: target.id.clone(),
        target_title: target.title.clone(),
        target_url: target.url.clone(),
        window_label: target.window_label.clone(),
        target_binding_status: target.binding_status.clone(),
        target_ownership_status: target.ownership_status.clone(),
        ownership_proven: target.ownership_proven,
        mutates_app,
        payload,
        test_id: test_id.clone(),
        step_id: step_id.clone(),
        telemetry_attributes: json!({
            "auditaur.test_id": test_id,
            "auditaur.step_id": step_id,
            "auditaur.driver.action": action,
            "auditaur.driver.selector": selector,
            "auditaur.driver.visible_only": visible_only,
            "auditaur.driver.target_id": target.id,
            "auditaur.driver.target_binding_status": target.binding_status,
            "auditaur.driver.target_ownership_status": target.ownership_status,
            "auditaur.driver.ownership_proven": target.ownership_proven,
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
            name: "exists",
            selector_required: true,
            mutates_app: false,
            description: "assert that a selector exists immediately",
        },
        DriverActionSpec {
            name: "text",
            selector_required: true,
            mutates_app: false,
            description: "read text from a selector",
        },
        DriverActionSpec {
            name: "wait",
            selector_required: true,
            mutates_app: false,
            description: "wait for a selector to appear through CDP Runtime.evaluate",
        },
        DriverActionSpec {
            name: "screenshot",
            selector_required: false,
            mutates_app: false,
            description: "capture a PNG screenshot through CDP Page.captureScreenshot",
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
            name: "type",
            selector_required: true,
            mutates_app: true,
            description: "insert text through CDP input events after focusing an editable element",
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
        "auditaur.driver.target_binding_status",
        "auditaur.driver.target_ownership_status",
        "auditaur.driver.ownership_proven",
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
    println!(
        "Target ownership: {} - {}",
        table_cell(&info.cdp.target_ownership_status, 40),
        table_cell(&info.cdp.target_ownership_note, 180)
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
    println!("TARGET\tTYPE\tTITLE\tURL\tWINDOW\tBINDING\tOWNERSHIP\tWEBSOCKET");
    for target in targets {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            table_cell(&target.id, 80),
            table_cell(target.target_type.as_deref().unwrap_or("-"), 24),
            table_cell(target.title.as_deref().unwrap_or("-"), 80),
            table_cell(target.url.as_deref().unwrap_or("-"), 120),
            table_cell(target.window_label.as_deref().unwrap_or("-"), 80),
            table_cell(&target.binding_status, 40),
            table_cell(&target.ownership_status, 40),
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

fn print_action_result(result: &ActionResult) -> Result<()> {
    println!(
        "{} {} on target {}",
        result.action,
        if result.ok { "ok" } else { "failed" },
        table_cell(&result.target_id, 80)
    );
    if let Some(selector) = &result.selector {
        println!("Selector: {}", table_cell(selector, 120));
    }
    if let Some(window_label) = &result.window_label {
        println!("Window: {}", table_cell(window_label, 80));
    }
    println!("Payload: {}", table_cell(&result.payload.to_string(), 240));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{auto_probe_reason, parse_ws_endpoint, selector_expression};

    #[test]
    fn selector_expression_escapes_css_selector_as_javascript_string() {
        let expression = selector_expression(r#"[data-testid="save"]"#, false).unwrap();
        assert!(expression.contains(r#"const selector = "[data-testid=\"save\"]""#));
        assert!(expression.contains("const visibleOnly = false"));
        assert!(expression.contains("document.querySelectorAll(selector)"));
    }

    #[test]
    fn parses_ws_endpoint_host_and_port() {
        assert_eq!(
            parse_ws_endpoint("ws://127.0.0.1:9222/devtools/page/1").unwrap(),
            ("127.0.0.1".to_string(), 9222)
        );
    }

    #[test]
    fn auto_probe_reason_summarizes_multiple_port_failures() {
        let reason = auto_probe_reason(&[
            "port 9222 refused".to_string(),
            "port 9223 timed out".to_string(),
            "port 9224 invalid JSON".to_string(),
            "port 9225 refused".to_string(),
        ]);
        assert!(reason.contains("port 9222 refused"));
        assert!(reason.contains("port 9223 timed out"));
        assert!(reason.contains("port 9224 invalid JSON"));
        assert!(reason.contains("plus 1 more"));
    }
}
