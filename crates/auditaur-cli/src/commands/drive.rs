use std::{
    fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use auditaur_core::drive_bridge::{
    DriveBridgeRequest, DriveBridgeResponse, DriveBridgeStatus, DRIVE_BRIDGE_DIR,
    DRIVE_BRIDGE_IN_FLIGHT_DIR, DRIVE_BRIDGE_PROTOCOL_VERSION, DRIVE_BRIDGE_REQUESTS_DIR,
    DRIVE_BRIDGE_RESPONSES_DIR, DRIVE_BRIDGE_STATUS_FILE,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    commands::read,
    discovery::{self, DiscoveredApp, DiscoveryStatus},
    output::table_cell,
};

const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(100);
const FAILURE_SNAPSHOT_TEXT_LIMIT_CHARS: usize = 64 * 1024;
const DRIVE_BRIDGE_POLL_INTERVAL: Duration = Duration::from_millis(50);
const DRIVE_BRIDGE_ACTION_TIMEOUT: Duration = Duration::from_secs(30);
const DRIVE_BRIDGE_ACTIVE_WINDOW: i64 = 120_000_000_000;

#[cfg(target_os = "macos")]
const CURRENT_PLATFORM: &str = "macos";
#[cfg(target_os = "windows")]
const CURRENT_PLATFORM: &str = "windows";
#[cfg(all(unix, not(target_os = "macos")))]
const CURRENT_PLATFORM: &str = "unix";
#[cfg(not(any(unix, target_os = "windows")))]
const CURRENT_PLATFORM: &str = "unknown";

pub fn run(selector: DriveAppSelector, _cdp_port: Option<u16>, json: bool) -> Result<()> {
    let target = resolve_target(&selector)?;
    let attach = DriveAttachInfo::discover(target)?;
    read::print_json_or_table(json, &attach, || print_attach_info(&attach, false))
}

pub fn inspect(selector: DriveAppSelector, _cdp_port: Option<u16>, json: bool) -> Result<()> {
    let target = resolve_target(&selector)?;
    let attach = DriveAttachInfo::discover(target)?;
    read::print_json_or_table(json, &attach, || print_attach_info(&attach, true))
}

pub fn wait(
    selector: DriveAppSelector,
    _cdp_port: Option<u16>,
    options: WaitOptions,
) -> Result<()> {
    bridge_wait(selector, options)
}

pub fn exists(
    selector: DriveAppSelector,
    _cdp_port: Option<u16>,
    options: SelectorActionOptions,
) -> Result<()> {
    bridge_selector_action(selector, options, "exists", None, false)
}

pub fn text(
    selector: DriveAppSelector,
    _cdp_port: Option<u16>,
    options: SelectorActionOptions,
) -> Result<()> {
    bridge_selector_action(selector, options, "text", None, false)
}

pub fn click(
    selector: DriveAppSelector,
    _cdp_port: Option<u16>,
    options: SelectorActionOptions,
) -> Result<()> {
    bridge_selector_action(selector, options, "click", None, true)
}

pub fn fill(
    selector: DriveAppSelector,
    _cdp_port: Option<u16>,
    options: FillOptions,
) -> Result<()> {
    let selector_options = SelectorActionOptions {
        selector: options.selector,
        target_id: options.target_id,
        test_id: options.test_id,
        step_id: options.step_id,
        visible_only: options.visible_only,
        json: options.json,
    };
    bridge_selector_action(
        selector,
        selector_options,
        "fill",
        Some(options.value),
        true,
    )
}

pub fn type_text(
    selector: DriveAppSelector,
    _cdp_port: Option<u16>,
    options: TypeOptions,
) -> Result<()> {
    let selector_options = SelectorActionOptions {
        selector: options.selector,
        target_id: options.target_id,
        test_id: options.test_id,
        step_id: options.step_id,
        visible_only: options.visible_only,
        json: options.json,
    };
    bridge_selector_action(
        selector,
        selector_options,
        "type",
        Some(options.value),
        true,
    )
}

pub fn press(
    selector: DriveAppSelector,
    _cdp_port: Option<u16>,
    options: PressOptions,
) -> Result<()> {
    let selector_options = SelectorActionOptions {
        selector: options
            .selector
            .unwrap_or_else(|| "<active-element>".to_string()),
        target_id: options.target_id,
        test_id: options.test_id,
        step_id: options.step_id,
        visible_only: false,
        json: options.json,
    };
    bridge_selector_action(selector, selector_options, "press", Some(options.key), true)
}

pub fn hover(
    selector: DriveAppSelector,
    _cdp_port: Option<u16>,
    options: SelectorActionOptions,
) -> Result<()> {
    bridge_selector_action(selector, options, "hover", None, true)
}

pub fn select(
    selector: DriveAppSelector,
    _cdp_port: Option<u16>,
    options: SelectOptions,
) -> Result<()> {
    let selector_options = SelectorActionOptions {
        selector: options.selector,
        target_id: options.target_id,
        test_id: options.test_id,
        step_id: options.step_id,
        visible_only: options.visible_only,
        json: options.json,
    };
    bridge_selector_values_action(selector, selector_options, "select", options.values, true)
}

pub fn check(
    selector: DriveAppSelector,
    _cdp_port: Option<u16>,
    options: SelectorActionOptions,
) -> Result<()> {
    bridge_selector_action(selector, options, "check", None, true)
}

pub fn uncheck(
    selector: DriveAppSelector,
    _cdp_port: Option<u16>,
    options: SelectorActionOptions,
) -> Result<()> {
    bridge_selector_action(selector, options, "uncheck", None, true)
}

pub fn evaluate(
    selector: DriveAppSelector,
    _cdp_port: Option<u16>,
    options: EvaluateOptions,
) -> Result<()> {
    bridge_evaluate(selector, options)
}

pub fn screenshot(
    selector: DriveAppSelector,
    _cdp_port: Option<u16>,
    options: ScreenshotOptions,
) -> Result<()> {
    bridge_screenshot(selector, options)
}

pub fn snapshot(
    selector: DriveAppSelector,
    _cdp_port: Option<u16>,
    options: SnapshotOptions,
) -> Result<()> {
    let selector_options = SelectorActionOptions {
        selector: options
            .selector
            .clone()
            .unwrap_or_else(|| "body".to_string()),
        target_id: options.target_id,
        test_id: options.test_id,
        step_id: options.step_id,
        visible_only: false,
        json: options.json,
    };
    let result =
        bridge_selector_action_result(selector, selector_options, "snapshot", None, false)?;
    if let Some(output) = &options.output {
        fs::write(output, serde_json::to_vec_pretty(&result.payload)?)?;
    }
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
    pub json: bool,
}

#[derive(Debug)]
pub struct SelectOptions {
    pub selector: String,
    pub values: Vec<String>,
    pub target_id: Option<String>,
    pub test_id: Option<String>,
    pub step_id: Option<String>,
    pub visible_only: bool,
    pub json: bool,
}

#[derive(Debug)]
pub struct EvaluateOptions {
    pub expression: String,
    pub target_id: Option<String>,
    pub test_id: Option<String>,
    pub step_id: Option<String>,
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

#[derive(Debug)]
pub struct SnapshotOptions {
    pub output: Option<PathBuf>,
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
    bridge: BridgeAttachInfo,
    platform_backend: PlatformDriveBackend,
    future_actions: Vec<DriverActionSpec>,
    required_action_telemetry: Vec<&'static str>,
    note: String,
}

impl DriveAttachInfo {
    fn discover(app: DiscoveredApp) -> Result<Self> {
        let cdp = unavailable_cdp(
            None,
            "CDP probing is not used by Auditaur drive; selector actions run through the Tauri-native in-app driver.".to_string(),
        );
        let bridge = BridgeAttachInfo::discover(&app);
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
            bridge,
            platform_backend: PlatformDriveBackend::current(),
            future_actions: future_actions(),
            required_action_telemetry: required_action_telemetry(),
            note: "Drive is an optional app-driver layer; it observes Auditaur discovery metadata and sends bounded requests through the Tauri-native in-app driver instead of CDP.".to_string(),
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BridgeAttachInfo {
    status: String,
    protocol_version: u8,
    active: bool,
    reason: Option<String>,
    window_label: Option<String>,
    last_heartbeat_unix_nanos: Option<i64>,
    request_dir: String,
    response_dir: String,
    targets: Vec<CdpTarget>,
    guidance: String,
}

impl BridgeAttachInfo {
    fn discover(app: &DiscoveredApp) -> Self {
        let bridge_dir = bridge_dir_for_app(app);
        let request_dir = bridge_dir.join(DRIVE_BRIDGE_REQUESTS_DIR);
        let response_dir = bridge_dir.join(DRIVE_BRIDGE_RESPONSES_DIR);
        let status_path = bridge_dir.join(DRIVE_BRIDGE_STATUS_FILE);
        let guidance = "Enable the Auditaur frontend drive bridge explicitly with initAuditaur({ driveBridge: true }) in exactly one debug/dev WebView per Auditaur session, then rerun the drive command.".to_string();
        let status = fs::read(&status_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<DriveBridgeStatus>(&bytes).ok());
        let Some(status) = status else {
            return Self {
                status: "inactive".to_string(),
                protocol_version: DRIVE_BRIDGE_PROTOCOL_VERSION,
                active: false,
                reason: Some("No active frontend drive bridge heartbeat was found.".to_string()),
                window_label: None,
                last_heartbeat_unix_nanos: None,
                request_dir: request_dir.to_string_lossy().to_string(),
                response_dir: response_dir.to_string_lossy().to_string(),
                targets: Vec::new(),
                guidance,
            };
        };
        let age = now_unix_nanos().saturating_sub(status.last_heartbeat_unix_nanos);
        let protocol_supported = status.protocol_version == DRIVE_BRIDGE_PROTOCOL_VERSION;
        let heartbeat_fresh = age <= DRIVE_BRIDGE_ACTIVE_WINDOW;
        let actionable = status.active && protocol_supported;
        let mut bridge = Self {
            status: if actionable && heartbeat_fresh {
                "active"
            } else if actionable {
                "stale"
            } else {
                "inactive"
            }
            .to_string(),
            protocol_version: status.protocol_version,
            active: actionable,
            reason: if !protocol_supported {
                Some(format!(
                    "Drive bridge protocol version {} is not supported by this CLI.",
                    status.protocol_version
                ))
            } else if !status.active {
                Some("Frontend drive bridge is not active.".to_string())
            } else if !heartbeat_fresh {
                Some(
                    "Frontend drive bridge heartbeat is stale; drive actions will attempt the native request wake path."
                        .to_string(),
                )
            } else {
                None
            },
            window_label: status.window_label,
            last_heartbeat_unix_nanos: Some(status.last_heartbeat_unix_nanos),
            request_dir: request_dir.to_string_lossy().to_string(),
            response_dir: response_dir.to_string_lossy().to_string(),
            targets: Vec::new(),
            guidance,
        };
        if actionable {
            bridge.targets.push(CdpTarget {
                id: "auditaur-bridge".to_string(),
                target_type: Some("bridge".to_string()),
                title: Some("Auditaur in-app drive bridge".to_string()),
                url: None,
                web_socket_debugger_url: None,
                binding_status: "matched_session_bridge".to_string(),
                binding_reason: Some(
                    "Drive bridge request/response queue belongs to the observed Auditaur session."
                        .to_string(),
                ),
                window_label: bridge.window_label.clone(),
                webview_label: bridge.window_label.clone(),
                ownership_status: "proven_session_bridge".to_string(),
                ownership_proof: Some("auditaur_session_directory".to_string()),
                ownership_proven: true,
                ownership_guidance: None,
            });
        }
        bridge
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlatformDriveBackend {
    platform: &'static str,
    webview_engine: &'static str,
    selector_backend: &'static str,
    status: &'static str,
    selector_actions_supported: bool,
    guidance: String,
    fallback: &'static str,
}

impl PlatformDriveBackend {
    fn current() -> Self {
        match CURRENT_PLATFORM {
            "macos" => Self {
                platform: "macos",
                webview_engine: "WKWebView",
                selector_backend: "tauri_in_app_driver",
                status: "supported_with_drive_bridge",
                selector_actions_supported: true,
                guidance: "macOS Tauri apps use WKWebView, so Auditaur drive uses the explicit Tauri-native in-app driver instead of CDP.".to_string(),
                fallback: "none",
            },
            "windows" => Self {
                platform: "windows",
                webview_engine: "WebView2",
                selector_backend: "tauri_in_app_driver",
                status: "supported_with_drive_bridge",
                selector_actions_supported: true,
                guidance: "Auditaur drive uses the explicit Tauri-native in-app driver; WebView2 remote debugging is not required.".to_string(),
                fallback: "none",
            },
            _ => Self {
                platform: CURRENT_PLATFORM,
                webview_engine: "unknown",
                selector_backend: "tauri_in_app_driver",
                status: "supported_with_drive_bridge",
                selector_actions_supported: true,
                guidance: "Auditaur drive uses the explicit Tauri-native in-app driver; a browser debugging protocol is not required.".to_string(),
                fallback: "none",
            },
        }
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

fn bridge_wait(selector: DriveAppSelector, options: WaitOptions) -> Result<()> {
    if options
        .target_id
        .as_deref()
        .is_some_and(|target| target != "auditaur-bridge")
    {
        return Err(anyhow!(
            "The Auditaur drive bridge only supports target `auditaur-bridge`."
        ));
    }
    let target = resolve_target(&selector)?;
    let attach = DriveAttachInfo::discover(target)?;
    require_bridge_active(&attach)?;
    let bridge_target = bridge_target(&attach);
    let started = Instant::now();
    let timeout = Duration::from_millis(options.timeout_ms);
    let mut matched = false;
    while started.elapsed() <= timeout {
        let mut request = bridge_request(
            "exists",
            Some(options.selector.clone()),
            None,
            options.visible_only,
            options.test_id.clone(),
            options.step_id.clone(),
        );
        request.window_label = attach.bridge.window_label.clone();
        let response = execute_bridge_request(&attach, &request, DRIVE_BRIDGE_ACTION_TIMEOUT)?;
        matched = response
            .payload
            .get("exists")
            .and_then(Value::as_bool)
            .unwrap_or(response.ok);
        if matched {
            break;
        }
        thread::sleep(WAIT_POLL_INTERVAL);
    }
    let wait_result = wait_result(
        &attach,
        &bridge_target,
        &options,
        matched,
        started.elapsed().as_millis(),
    );
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

fn bridge_selector_action(
    selector: DriveAppSelector,
    options: SelectorActionOptions,
    action: &str,
    value: Option<String>,
    mutates_app: bool,
) -> Result<()> {
    let json = options.json;
    let result = bridge_selector_action_result(selector, options, action, value, mutates_app)?;
    let ok = result.ok;
    let error = result
        .payload
        .get("error")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    read::print_json_or_table(json, &result, || print_action_result(&result))?;
    if ok {
        Ok(())
    } else {
        Err(anyhow!(
            "drive {action} failed: {}",
            error.unwrap_or_else(|| "bridge action returned ok=false".to_string())
        ))
    }
}

fn bridge_selector_values_action(
    selector: DriveAppSelector,
    options: SelectorActionOptions,
    action: &str,
    values: Vec<String>,
    mutates_app: bool,
) -> Result<()> {
    let json = options.json;
    let result =
        bridge_selector_values_action_result(selector, options, action, values, mutates_app)?;
    let ok = result.ok;
    let error = result
        .payload
        .get("error")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    read::print_json_or_table(json, &result, || print_action_result(&result))?;
    if ok {
        Ok(())
    } else {
        Err(anyhow!(
            "drive {action} failed: {}",
            error.unwrap_or_else(|| "bridge action returned ok=false".to_string())
        ))
    }
}

fn bridge_selector_action_result(
    selector: DriveAppSelector,
    options: SelectorActionOptions,
    action: &str,
    value: Option<String>,
    mutates_app: bool,
) -> Result<ActionResult> {
    if options
        .target_id
        .as_deref()
        .is_some_and(|target| target != "auditaur-bridge")
    {
        return Err(anyhow!(
            "The Auditaur drive bridge only supports target `auditaur-bridge`."
        ));
    }
    let target = resolve_target(&selector)?;
    let attach = DriveAttachInfo::discover(target)?;
    require_bridge_active(&attach)?;
    let bridge_target = bridge_target(&attach);
    let request_selector = if action == "press" && options.selector.as_str() == "<active-element>" {
        None
    } else {
        Some(options.selector.clone())
    };
    let mut request = bridge_request(
        action,
        request_selector,
        value,
        options.visible_only,
        options.test_id.clone(),
        options.step_id.clone(),
    );
    request.window_label = attach.bridge.window_label.clone();
    let response = execute_bridge_request(&attach, &request, DRIVE_BRIDGE_ACTION_TIMEOUT)?;
    let mut payload = response.payload;
    if let Some(error) = response.error {
        if let Some(payload) = payload.as_object_mut() {
            payload.entry("error".to_string()).or_insert(json!(error));
        }
    }
    Ok(action_result(
        &attach,
        &bridge_target,
        action,
        Some(options.selector),
        options.visible_only,
        mutates_app,
        payload,
        &options.test_id,
        &options.step_id,
    ))
}

fn bridge_selector_values_action_result(
    selector: DriveAppSelector,
    options: SelectorActionOptions,
    action: &str,
    values: Vec<String>,
    mutates_app: bool,
) -> Result<ActionResult> {
    if options
        .target_id
        .as_deref()
        .is_some_and(|target| target != "auditaur-bridge")
    {
        return Err(anyhow!(
            "The Auditaur drive bridge only supports target `auditaur-bridge`."
        ));
    }
    let target = resolve_target(&selector)?;
    let attach = DriveAttachInfo::discover(target)?;
    require_bridge_active(&attach)?;
    let bridge_target = bridge_target(&attach);
    let mut request = bridge_request(
        action,
        Some(options.selector.clone()),
        values.first().cloned(),
        options.visible_only,
        options.test_id.clone(),
        options.step_id.clone(),
    );
    request.values = values;
    request.window_label = attach.bridge.window_label.clone();
    let response = execute_bridge_request(&attach, &request, DRIVE_BRIDGE_ACTION_TIMEOUT)?;
    let mut payload = response.payload;
    if let Some(error) = response.error {
        if let Some(payload) = payload.as_object_mut() {
            payload.entry("error".to_string()).or_insert(json!(error));
        }
    }
    Ok(action_result(
        &attach,
        &bridge_target,
        action,
        Some(options.selector),
        options.visible_only,
        mutates_app,
        payload,
        &options.test_id,
        &options.step_id,
    ))
}

fn bridge_evaluate(selector: DriveAppSelector, options: EvaluateOptions) -> Result<()> {
    if options
        .target_id
        .as_deref()
        .is_some_and(|target| target != "auditaur-bridge")
    {
        return Err(anyhow!(
            "The Auditaur drive bridge only supports target `auditaur-bridge`."
        ));
    }
    let target = resolve_target(&selector)?;
    let attach = DriveAttachInfo::discover(target)?;
    require_bridge_active(&attach)?;
    let bridge_target = bridge_target(&attach);
    let mut request = bridge_request(
        "evaluate",
        None,
        Some(options.expression),
        false,
        options.test_id.clone(),
        options.step_id.clone(),
    );
    request.window_label = attach.bridge.window_label.clone();
    let response = execute_bridge_request(&attach, &request, DRIVE_BRIDGE_ACTION_TIMEOUT)?;
    let mut payload = response.payload;
    if let Some(error) = response.error {
        if let Some(payload) = payload.as_object_mut() {
            payload.entry("error".to_string()).or_insert(json!(error));
        }
    }
    let result = action_result(
        &attach,
        &bridge_target,
        "evaluate",
        None,
        false,
        true,
        payload,
        &options.test_id,
        &options.step_id,
    );
    let ok = result.ok;
    let error = result
        .payload
        .get("error")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    read::print_json_or_table(options.json, &result, || print_action_result(&result))?;
    if ok {
        Ok(())
    } else {
        Err(anyhow!(
            "drive evaluate failed: {}",
            error.unwrap_or_else(|| "bridge action returned ok=false".to_string())
        ))
    }
}

fn bridge_screenshot(selector: DriveAppSelector, options: ScreenshotOptions) -> Result<()> {
    if options
        .target_id
        .as_deref()
        .is_some_and(|target| target != "auditaur-bridge")
    {
        return Err(anyhow!(
            "The Auditaur drive bridge only supports target `auditaur-bridge`."
        ));
    }
    let target = resolve_target(&selector)?;
    let attach = DriveAttachInfo::discover(target)?;
    require_bridge_active(&attach)?;
    let bridge_target = bridge_target(&attach);
    let mut request = bridge_request(
        "screenshot",
        options.selector.clone(),
        None,
        false,
        options.test_id.clone(),
        options.step_id.clone(),
    );
    request.window_label = attach.bridge.window_label.clone();
    let response = execute_bridge_request(&attach, &request, DRIVE_BRIDGE_ACTION_TIMEOUT)?;
    let mut payload = response.payload;
    let png_base64 = payload
        .get("pngBase64")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            let message = response
                .error
                .as_deref()
                .or_else(|| payload.get("error").and_then(Value::as_str))
                .unwrap_or("bridge response did not include pngBase64");
            anyhow!("drive screenshot failed: {message}")
        })?
        .to_string();
    let bytes = BASE64_STANDARD
        .decode(png_base64.as_bytes())
        .context("drive screenshot failed: bridge response had invalid pngBase64")?;
    fs::write(&options.output, bytes)?;
    if let Some(payload) = payload.as_object_mut() {
        payload.remove("pngBase64");
        payload.insert(
            "output".to_string(),
            json!(options.output.to_string_lossy().to_string()),
        );
        payload.insert("format".to_string(), json!("png"));
    }
    if let Some(snapshot_output) = &options.snapshot_output {
        let snapshot = payload.get("snapshot").cloned();
        let manifest = json!({
            "schemaVersion": 1,
            "action": "screenshot",
            "serviceName": attach.service_name.clone(),
            "sessionId": attach.session_id.clone(),
            "pid": attach.pid,
            "targetId": bridge_target.id.clone(),
            "targetTitle": bridge_target.title.clone(),
            "targetUrl": bridge_target.url.clone(),
            "windowLabel": bridge_target.window_label.clone(),
            "targetBindingStatus": bridge_target.binding_status.clone(),
            "targetOwnershipStatus": bridge_target.ownership_status.clone(),
            "ownershipProven": bridge_target.ownership_proven,
            "selector": options.selector.clone(),
            "testId": options.test_id.clone(),
            "stepId": options.step_id.clone(),
            "artifacts": {
                "screenshot": options.output.to_string_lossy(),
                "snapshot": snapshot_output.to_string_lossy(),
            },
            "snapshotTextLimitCharacters": FAILURE_SNAPSHOT_TEXT_LIMIT_CHARS,
            "snapshot": snapshot,
            "snapshotError": null,
        });
        fs::write(snapshot_output, serde_json::to_vec_pretty(&manifest)?)?;
        if let Some(payload) = payload.as_object_mut() {
            payload.insert(
                "snapshot".to_string(),
                json!(snapshot_output.to_string_lossy().to_string()),
            );
            payload.insert(
                "snapshotTextLimitCharacters".to_string(),
                json!(FAILURE_SNAPSHOT_TEXT_LIMIT_CHARS),
            );
        }
    }
    let result = action_result(
        &attach,
        &bridge_target,
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

fn require_bridge_active(attach: &DriveAttachInfo) -> Result<()> {
    if attach.bridge.active {
        return Ok(());
    }
    Err(anyhow!(
        "Auditaur drive bridge is not active for session {}. {}",
        attach.session_id,
        attach.bridge.guidance
    ))
}

fn bridge_request(
    action: &str,
    selector: Option<String>,
    value: Option<String>,
    visible_only: bool,
    test_id: Option<String>,
    step_id: Option<String>,
) -> DriveBridgeRequest {
    DriveBridgeRequest {
        schema_version: 1,
        protocol_version: DRIVE_BRIDGE_PROTOCOL_VERSION,
        request_id: format!("{}-{}", std::process::id(), now_unix_nanos()),
        action: action.to_string(),
        selector,
        value,
        values: Vec::new(),
        visible_only,
        window_label: None,
        test_id,
        step_id,
        created_at_unix_nanos: now_unix_nanos(),
    }
}

fn execute_bridge_request(
    attach: &DriveAttachInfo,
    request: &DriveBridgeRequest,
    timeout: Duration,
) -> Result<DriveBridgeResponse> {
    let request_dir = PathBuf::from(&attach.bridge.request_dir);
    let response_dir = PathBuf::from(&attach.bridge.response_dir);
    fs::create_dir_all(&request_dir)?;
    fs::create_dir_all(&response_dir)?;
    let request_path = request_dir.join(format!("{}.json", request.request_id));
    atomic_write_json(&request_path, request)?;
    let response_path = response_dir.join(format!("{}.json", request.request_id));
    let deadline = Instant::now() + timeout;
    while Instant::now() <= deadline {
        if response_path.exists() {
            let bytes = fs::read(&response_path)?;
            fs::remove_file(&response_path)?;
            return serde_json::from_slice(&bytes).map_err(Into::into);
        }
        thread::sleep(DRIVE_BRIDGE_POLL_INTERVAL);
    }
    let _ = fs::remove_file(&request_path);
    let _ = fs::remove_file(
        bridge_dir_for_attach(attach)
            .join(DRIVE_BRIDGE_IN_FLIGHT_DIR)
            .join(format!("{}.json", request.request_id)),
    );
    Err(anyhow!(
        "Timed out after {}ms waiting for Auditaur drive bridge response to `{}`.",
        timeout.as_millis(),
        request.action
    ))
}

fn bridge_target(attach: &DriveAttachInfo) -> CdpTarget {
    CdpTarget {
        id: "auditaur-bridge".to_string(),
        target_type: Some("bridge".to_string()),
        title: Some("Auditaur in-app drive bridge".to_string()),
        url: None,
        web_socket_debugger_url: None,
        binding_status: "matched_session_bridge".to_string(),
        binding_reason: Some(
            "Drive bridge request/response queue belongs to the observed Auditaur session."
                .to_string(),
        ),
        window_label: attach.bridge.window_label.clone(),
        webview_label: attach.bridge.window_label.clone(),
        ownership_status: "proven_session_bridge".to_string(),
        ownership_proof: Some("auditaur_session_directory".to_string()),
        ownership_proven: true,
        ownership_guidance: None,
    }
}

fn bridge_dir_for_app(app: &DiscoveredApp) -> PathBuf {
    Path::new(&app.database_path)
        .parent()
        .map(|path| path.join(DRIVE_BRIDGE_DIR))
        .unwrap_or_else(|| PathBuf::from(DRIVE_BRIDGE_DIR))
}

fn bridge_dir_for_attach(attach: &DriveAttachInfo) -> PathBuf {
    PathBuf::from(&attach.bridge.request_dir)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(DRIVE_BRIDGE_DIR))
}

fn now_unix_nanos() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(now.as_nanos()).unwrap_or(i64::MAX)
}

fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("atomic write target has no parent directory"))?;
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("atomic write target has no UTF-8 file name"))?;
    let temp_path = parent.join(format!(
        ".{file_name}.{}.tmp",
        now_unix_nanos().saturating_abs()
    ));
    fs::write(&temp_path, bytes)?;
    fs::rename(temp_path, path)?;
    Ok(())
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

fn launch_hint(cdp_port: Option<u16>) -> String {
    if let Some(port) = cdp_port {
        format!(
            "--cdp-port {port} is ignored by `auditaur drive`; enable driveBridge in the frontend and rerun without a browser debugging endpoint."
        )
    } else {
        "Auditaur drive uses the Tauri-native in-app driver; enable initAuditaur({ driveBridge: true }) in a debug/test WebView.".to_string()
    }
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
            description: "wait for a selector to appear through the Tauri-native in-app driver",
        },
        DriverActionSpec {
            name: "screenshot",
            selector_required: false,
            mutates_app: false,
            description: "capture a PNG screenshot through native window capture, falling back to a DOM text summary PNG",
        },
        DriverActionSpec {
            name: "hover",
            selector_required: true,
            mutates_app: true,
            description: "dispatch pointer/mouse hover events on a selector",
        },
        DriverActionSpec {
            name: "select",
            selector_required: true,
            mutates_app: true,
            description: "select one or more option values in a select element",
        },
        DriverActionSpec {
            name: "check",
            selector_required: true,
            mutates_app: true,
            description: "check a checkbox or radio input",
        },
        DriverActionSpec {
            name: "uncheck",
            selector_required: true,
            mutates_app: true,
            description: "uncheck a checkbox input",
        },
        DriverActionSpec {
            name: "evaluate",
            selector_required: false,
            mutates_app: true,
            description: "evaluate JavaScript in the WebView and return a serializable value",
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
            description: "insert text through in-WebView input events after focusing an editable element",
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
    println!(
        "Platform drive backend: {} / {} - {}",
        info.platform_backend.platform,
        info.platform_backend.webview_engine,
        table_cell(&info.platform_backend.guidance, 180)
    );
    println!(
        "Bridge: {} - {}",
        table_cell(&info.bridge.status, 40),
        table_cell(
            info.bridge
                .reason
                .as_deref()
                .unwrap_or("Auditaur in-app drive bridge is active."),
            180
        )
    );
    println!(
        "Target binding: {} - {}",
        table_cell("matched_session_bridge", 40),
        table_cell(
            "Drive requests target the observed Auditaur session through the Tauri-native bridge.",
            180
        )
    );
    println!(
        "Target ownership: {} - {}",
        table_cell("proven_session_bridge", 40),
        table_cell(
            "The in-app driver queue belongs to the observed Auditaur session directory.",
            180
        )
    );
    if show_targets {
        print_targets(&info.bridge.targets);
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
    use super::PlatformDriveBackend;

    #[test]
    fn platform_backend_reports_tauri_native_driver() {
        let backend = PlatformDriveBackend::current();
        if cfg!(target_os = "macos") {
            assert_eq!(backend.platform, "macos");
            assert_eq!(backend.webview_engine, "WKWebView");
            assert_eq!(backend.status, "supported_with_drive_bridge");
            assert_eq!(backend.selector_actions_supported, true);
            assert!(backend
                .guidance
                .contains("Tauri-native in-app driver instead of CDP"));
            assert_eq!(backend.fallback, "none");
        } else {
            assert_eq!(backend.selector_backend, "tauri_in_app_driver");
            assert_eq!(backend.status, "supported_with_drive_bridge");
            assert_eq!(backend.selector_actions_supported, true);
        }
    }
}
