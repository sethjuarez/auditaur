use std::{
    collections::HashSet,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

#[cfg(not(windows))]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

use anyhow::{anyhow, Context, Result};
use auditaur_core::{
    redaction::redact_json,
    resolve_data_dir,
    storage::{FrontendErrorQuery, TauriIpcQuery},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::{
    commands::{debug, drive, polish, read},
    discovery::{self, DiscoveredApp, DiscoveryStatus},
    output::bounded_text,
};

const EXIT_PASSED: i32 = 0;
const EXIT_CHECK_FAILED: i32 = 1;
const EXIT_RUNNER_ERROR: i32 = 2;
const EXIT_APP_EXITED: i32 = 3;
const EXIT_TIMEOUT: i32 = 4;
const EXIT_CLEANUP_FAILED: i32 = 5;
const DEFAULT_HOOK_TIMEOUT_MS: u64 = 30_000;
const HOOK_OUTPUT_MAX_CHARS: usize = 65_536;
const HOOK_OUTPUT_MAX_BYTES: usize = HOOK_OUTPUT_MAX_CHARS * 4;

#[derive(Debug)]
pub struct DrillRunOptions {
    pub app: String,
    pub require_frontend: bool,
    pub require_drive_bridge: bool,
    pub timeout_seconds: u64,
    pub interval_ms: u64,
    pub report: PathBuf,
    pub selector: Option<String>,
    pub expect_text: Option<String>,
    pub script: Option<PathBuf>,
    pub json: bool,
    pub environment: Vec<(String, String)>,
    pub command: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrillReport {
    schema_version: u8,
    generated_at_unix_nanos: i64,
    status: DrillStatus,
    exit_code: i32,
    app: DrillApp,
    command: DrillCommandReport,
    script: Option<DrillScriptReport>,
    options: DrillOptionsReport,
    phases: Vec<DrillPhase>,
    summary: DrillSummary,
    artifacts: Vec<DrillArtifact>,
    redaction: DrillRedaction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DrillStatus {
    Passed,
    Failed,
    Error,
    TimedOut,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrillApp {
    service_name: String,
    session_id: Option<String>,
    instance_id: Option<String>,
    pid: Option<u32>,
    database_path: Option<String>,
    started_at: Option<String>,
    pinned_by: Option<String>,
    preexisting_matching_sessions: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrillCommandReport {
    argv: Vec<String>,
    root_pid: Option<u32>,
    exit_code: Option<i32>,
    running_at_readiness: Option<bool>,
    timed_out: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrillOptionsReport {
    require_frontend: bool,
    require_drive_bridge: bool,
    timeout_seconds: u64,
    selector: Option<String>,
    expect_text: Option<String>,
    script: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DrillScript {
    #[serde(default)]
    setup: Vec<DrillHook>,
    #[serde(default)]
    gates: Vec<DrillGate>,
    #[serde(default)]
    teardown: Vec<DrillHook>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DrillGate {
    name: String,
    instructions: String,
    #[serde(default)]
    selector: Option<String>,
    #[serde(default)]
    expect_text: Option<String>,
    #[serde(default = "default_gate_timeout_ms")]
    timeout_ms: u64,
    #[serde(default = "default_manual_continue")]
    manual_continue: bool,
    #[serde(default)]
    choices: Vec<DrillGateChoice>,
    #[serde(default)]
    inputs: Vec<DrillGateInput>,
    #[serde(default)]
    clipboard: Option<DrillGateClipboard>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DrillGateClipboard {
    label: String,
    value: String,
    #[serde(default = "default_sensitive_input")]
    sensitive: bool,
    #[serde(default = "default_clipboard_copy_mode")]
    copy: DrillGateClipboardCopyMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum DrillGateClipboardCopyMode {
    Attempt,
    ManualOnly,
    Disabled,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DrillGateChoice {
    id: String,
    label: String,
    #[serde(default = "default_gate_choice_outcome")]
    outcome: DrillGateChoiceOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum DrillGateChoiceOutcome {
    Continue,
    Retry,
    Skip,
    Fail,
    Abort,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DrillGateInput {
    id: String,
    label: String,
    #[serde(default)]
    kind: DrillGateInputKind,
    #[serde(default)]
    required: bool,
    #[serde(default = "default_sensitive_input")]
    sensitive: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum DrillGateInputKind {
    Text,
    MultilineText,
}

impl Default for DrillGateInputKind {
    fn default() -> Self {
        Self::Text
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DrillHook {
    name: String,
    run: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default = "default_hook_timeout_ms")]
    timeout_ms: u64,
    #[serde(default = "default_hook_cwd")]
    cwd: PathBuf,
    #[serde(default)]
    always: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrillScriptReport {
    path: String,
    workspace_root: String,
    setup: Vec<DrillHookReport>,
    gates: Vec<DrillGateReport>,
    teardown: Vec<DrillHookReport>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrillGateReport {
    name: String,
    instructions: String,
    selector: Option<String>,
    expect_text: Option<String>,
    timeout_ms: u64,
    manual_continue: bool,
    duration_ms: u128,
    satisfied_by: DrillGateSatisfiedBy,
    observed_text: Option<String>,
    selected_choice: Option<DrillGateChoice>,
    responder: Option<String>,
    inputs: Vec<DrillGateInputReport>,
    clipboard: Option<DrillGateClipboardReport>,
}

#[derive(Debug, Clone)]
struct PublishedGateRequest {
    request_id: String,
    request_path: PathBuf,
    response_path: PathBuf,
    cleanup_paths: Vec<PathBuf>,
    nonce: String,
    run_id: String,
    gate_id: String,
    attempt: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GateQueueResponse {
    schema_version: u8,
    request_id: String,
    run_id: String,
    gate_id: String,
    attempt: u32,
    nonce: String,
    #[serde(default)]
    responder: Option<String>,
    #[serde(default)]
    choice_id: Option<String>,
    #[serde(default)]
    inputs: Vec<GateQueueInputValue>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GateQueueInputValue {
    id: String,
    value: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrillGateClipboardReport {
    label: String,
    sensitive: bool,
    redacted: bool,
    value: Option<String>,
    copy: DrillGateClipboardCopyMode,
    status: DrillGateClipboardStatus,
    error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum DrillGateClipboardStatus {
    Copied,
    Failed,
    ManualOnly,
    Disabled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrillGateInputReport {
    id: String,
    label: String,
    kind: DrillGateInputKind,
    required: bool,
    sensitive: bool,
    redacted: bool,
    value: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DrillGateSatisfiedBy {
    SelectorText,
    ManualContinue,
    Choice,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrillHookReport {
    name: String,
    lifecycle: DrillHookLifecycle,
    command: String,
    args: Vec<String>,
    cwd: String,
    timeout_ms: u64,
    always: bool,
    exit_code: Option<i32>,
    duration_ms: u128,
    timed_out: bool,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum DrillHookLifecycle {
    Setup,
    Teardown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookExitKind {
    Passed,
    Failed,
    TimedOut,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrillPhase {
    id: String,
    kind: String,
    status: DrillPhaseStatus,
    duration_ms: u128,
    message: String,
    result: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DrillPhaseStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrillSummary {
    passed: usize,
    failed: usize,
    skipped: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrillArtifact {
    kind: String,
    path: String,
    redacted: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrillRedaction {
    defaults: bool,
    extra_keys: Vec<String>,
    redacted: bool,
}

pub fn run(options: DrillRunOptions) -> Result<i32> {
    let started_at = OffsetDateTime::now_utc();
    let deadline = Instant::now() + Duration::from_secs(options.timeout_seconds);
    let interval = Duration::from_millis(options.interval_ms).max(Duration::from_millis(100));
    let preexisting = matching_sessions(&options.app)?;
    let mut report = DrillReport::new(&options, &preexisting);
    let mut exit_code = EXIT_PASSED;
    let script = match load_script(&options) {
        Ok(script) => script,
        Err(error) => {
            exit_code = EXIT_RUNNER_ERROR;
            report.push_phase(DrillPhase::failed(
                "script",
                "scriptConfig",
                Instant::now(),
                format!("failed to load drill script: {error}"),
                None,
            ));
            return finish(report, &options, exit_code);
        }
    };
    if let Some(script) = &script {
        report.set_script(script);
    }

    if let Some(script) = &script {
        if run_hooks(
            script,
            DrillHookLifecycle::Setup,
            &mut report,
            &mut exit_code,
        )
        .is_err()
        {
            run_teardown_hooks(&script, &mut report, &mut exit_code);
            return finish(report, &options, exit_code);
        }
    }

    let spawn_start = Instant::now();
    let mut child = match spawn_command(&options.command, &options.environment) {
        Ok(child) => {
            let root_pid = child.id();
            report.command.root_pid = Some(root_pid);
            report.push_phase(DrillPhase::passed(
                "spawn",
                "process",
                spawn_start,
                format!("started wrapped command as pid {root_pid}"),
                Some(json!({ "pid": root_pid })),
            ));
            Some(child)
        }
        Err(error) => {
            exit_code = EXIT_RUNNER_ERROR;
            report.push_phase(DrillPhase::failed(
                "spawn",
                "process",
                spawn_start,
                format!("failed to start wrapped command: {error}"),
                None,
            ));
            if let Some(script) = &script {
                run_teardown_hooks(script, &mut report, &mut exit_code);
            }
            return finish(report, &options, exit_code);
        }
    };

    let pinned = wait_for_spawned_session(
        &options.app,
        started_at,
        &preexisting,
        deadline,
        interval,
        child.as_mut(),
        &mut report,
    )?;

    if let Some(app) = pinned {
        report.pin_app(&app);
        match wait_for_debug_ready(
            &options,
            &app,
            deadline,
            interval,
            child.as_mut(),
            &mut report,
        )? {
            WaitOutcome::Ready(status) => {
                report.command.running_at_readiness = match child.as_mut() {
                    Some(child) => child.try_wait()?.map(|_| false).or(Some(true)),
                    None => None,
                };
                report.push_phase(DrillPhase::passed(
                    "debug-readiness",
                    "debugStatus",
                    Instant::now(),
                    "debug readiness reached for spawned session",
                    Some(serde_json::to_value(status)?),
                ));
                let gates_passed = if let Some(script) = &script {
                    run_gates(script, &app, child.as_mut(), &mut report, &mut exit_code)?
                } else {
                    true
                };
                if gates_passed {
                    run_post_readiness_phases(&options, &app, &mut report, &mut exit_code);
                }
            }
            WaitOutcome::AppExited(code) => {
                exit_code = EXIT_APP_EXITED;
                report.command.exit_code = code;
                report.push_phase(DrillPhase::failed(
                    "debug-readiness",
                    "debugStatus",
                    Instant::now(),
                    "wrapped command exited before debug readiness",
                    Some(json!({ "exitCode": code })),
                ));
            }
            WaitOutcome::TimedOut(last_status) => {
                exit_code = EXIT_TIMEOUT;
                report.command.timed_out = true;
                report.push_phase(DrillPhase::failed(
                    "debug-readiness",
                    "debugStatus",
                    Instant::now(),
                    format!(
                        "timed out after {}s waiting for debug readiness",
                        options.timeout_seconds
                    ),
                    last_status
                        .map(|status| serde_json::to_value(status))
                        .transpose()?,
                ));
            }
        }
    } else {
        exit_code = if Instant::now() >= deadline {
            EXIT_TIMEOUT
        } else {
            EXIT_APP_EXITED
        };
    }

    if let Some(mut child) = child {
        let cleanup = cleanup_child_tree(&mut child);
        match cleanup {
            Ok(message) => report.push_phase(DrillPhase::passed(
                "cleanup",
                "processTree",
                Instant::now(),
                message,
                Some(json!({ "rootPid": report.command.root_pid })),
            )),
            Err(error) => {
                if exit_code == EXIT_PASSED {
                    exit_code = EXIT_CLEANUP_FAILED;
                }
                report.push_phase(DrillPhase::failed(
                    "cleanup",
                    "processTree",
                    Instant::now(),
                    format!("failed to clean up process tree: {error}"),
                    Some(json!({ "rootPid": report.command.root_pid })),
                ));
            }
        }
    }

    if let Some(script) = &script {
        run_teardown_hooks(script, &mut report, &mut exit_code);
    }

    finish(report, &options, exit_code)
}

fn load_script(options: &DrillRunOptions) -> Result<Option<ResolvedDrillScript>> {
    let Some(path) = &options.script else {
        return Ok(None);
    };
    let workspace_root = std::env::current_dir()
        .context("failed to resolve current workspace")?
        .canonicalize()
        .context("failed to canonicalize current workspace")?;
    let script_path = resolve_existing_path(&workspace_root, path)
        .with_context(|| format!("invalid script path `{}`", path.display()))?;
    let contents = fs::read_to_string(&script_path)
        .with_context(|| format!("failed to read `{}`", script_path.display()))?;
    let script: DrillScript = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse `{}`", script_path.display()))?;
    let setup = resolve_hooks(&workspace_root, script.setup, DrillHookLifecycle::Setup)?;
    let gates = resolve_gates(script.gates)?;
    let teardown = resolve_hooks(
        &workspace_root,
        script.teardown,
        DrillHookLifecycle::Teardown,
    )?;
    Ok(Some(ResolvedDrillScript {
        path: script_path,
        workspace_root,
        setup,
        gates,
        teardown,
    }))
}

#[derive(Debug)]
struct ResolvedDrillScript {
    path: PathBuf,
    workspace_root: PathBuf,
    setup: Vec<ResolvedDrillHook>,
    gates: Vec<ResolvedDrillGate>,
    teardown: Vec<ResolvedDrillHook>,
}

#[derive(Debug)]
struct ResolvedDrillGate {
    name: String,
    instructions: String,
    selector: Option<String>,
    expect_text: Option<String>,
    timeout_ms: u64,
    manual_continue: bool,
    choices: Vec<DrillGateChoice>,
    inputs: Vec<DrillGateInput>,
    clipboard: Option<DrillGateClipboard>,
}

#[derive(Debug)]
struct ResolvedDrillHook {
    name: String,
    run: String,
    args: Vec<String>,
    timeout_ms: u64,
    cwd: PathBuf,
    always: bool,
}

fn resolve_gates(gates: Vec<DrillGate>) -> Result<Vec<ResolvedDrillGate>> {
    gates.into_iter().map(resolve_gate).collect()
}

fn resolve_gate(gate: DrillGate) -> Result<ResolvedDrillGate> {
    if gate.name.trim().is_empty() {
        return Err(anyhow!("gate name must not be empty"));
    }
    if gate.instructions.trim().is_empty() {
        return Err(anyhow!(
            "gate `{}` instructions must not be empty",
            gate.name
        ));
    }
    if gate.timeout_ms == 0 {
        return Err(anyhow!(
            "gate `{}` timeoutMs must be greater than 0",
            gate.name
        ));
    }
    if gate.expect_text.is_some() && gate.selector.is_none() {
        return Err(anyhow!("gate `{}` expectText requires selector", gate.name));
    }
    if gate.selector.is_none()
        && !gate.manual_continue
        && gate.choices.is_empty()
        && gate.inputs.is_empty()
    {
        return Err(anyhow!(
            "gate `{}` must enable manualContinue, define choices or inputs, or provide selector",
            gate.name
        ));
    }
    for choice in &gate.choices {
        if choice.id.trim().is_empty() {
            return Err(anyhow!("gate `{}` choice id must not be empty", gate.name));
        }
        if choice.label.trim().is_empty() {
            return Err(anyhow!(
                "gate `{}` choice `{}` label must not be empty",
                gate.name,
                choice.id
            ));
        }
    }
    let mut choice_ids = HashSet::new();
    for choice in &gate.choices {
        if !choice_ids.insert(choice.id.as_str()) {
            return Err(anyhow!(
                "gate `{}` choice id `{}` must be unique",
                gate.name,
                choice.id
            ));
        }
    }
    for input in &gate.inputs {
        if input.id.trim().is_empty() {
            return Err(anyhow!("gate `{}` input id must not be empty", gate.name));
        }
        if input.label.trim().is_empty() {
            return Err(anyhow!(
                "gate `{}` input `{}` label must not be empty",
                gate.name,
                input.id
            ));
        }
    }
    let mut input_ids = HashSet::new();
    for input in &gate.inputs {
        if !input_ids.insert(input.id.as_str()) {
            return Err(anyhow!(
                "gate `{}` input id `{}` must be unique",
                gate.name,
                input.id
            ));
        }
    }
    if let Some(clipboard) = &gate.clipboard {
        if clipboard.label.trim().is_empty() {
            return Err(anyhow!(
                "gate `{}` clipboard label must not be empty",
                gate.name
            ));
        }
        if clipboard.value.is_empty() {
            return Err(anyhow!(
                "gate `{}` clipboard value must not be empty",
                gate.name
            ));
        }
    }
    Ok(ResolvedDrillGate {
        name: gate.name,
        instructions: gate.instructions,
        selector: gate.selector,
        expect_text: gate.expect_text,
        timeout_ms: gate.timeout_ms,
        manual_continue: gate.manual_continue,
        choices: gate.choices,
        inputs: gate.inputs,
        clipboard: gate.clipboard,
    })
}

fn resolve_hooks(
    workspace_root: &Path,
    hooks: Vec<DrillHook>,
    lifecycle: DrillHookLifecycle,
) -> Result<Vec<ResolvedDrillHook>> {
    hooks
        .into_iter()
        .map(|hook| resolve_hook(workspace_root, hook, lifecycle))
        .collect()
}

fn resolve_hook(
    workspace_root: &Path,
    hook: DrillHook,
    lifecycle: DrillHookLifecycle,
) -> Result<ResolvedDrillHook> {
    if hook.timeout_ms == 0 {
        return Err(anyhow!(
            "{} hook `{}` timeoutMs must be greater than 0",
            lifecycle_name(lifecycle),
            hook.name
        ));
    }
    if hook.run.trim().is_empty() {
        return Err(anyhow!(
            "{} hook `{}` run must not be empty",
            lifecycle_name(lifecycle),
            hook.name
        ));
    }
    let cwd = resolve_existing_path(workspace_root, &hook.cwd).with_context(|| {
        format!(
            "{} hook `{}` has invalid cwd `{}`",
            lifecycle_name(lifecycle),
            hook.name,
            hook.cwd.display()
        )
    })?;
    if !cwd.is_dir() {
        return Err(anyhow!(
            "{} hook `{}` cwd `{}` is not a directory",
            lifecycle_name(lifecycle),
            hook.name,
            cwd.display()
        ));
    }
    Ok(ResolvedDrillHook {
        name: hook.name,
        run: hook.run,
        args: hook.args,
        timeout_ms: hook.timeout_ms,
        cwd,
        always: hook.always,
    })
}

fn resolve_existing_path(workspace_root: &Path, path: &Path) -> Result<PathBuf> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    };
    let resolved = joined.canonicalize().with_context(|| {
        format!(
            "failed to canonicalize `{}` relative to `{}`",
            path.display(),
            workspace_root.display()
        )
    })?;
    if !resolved.starts_with(workspace_root) {
        return Err(anyhow!(
            "`{}` resolves outside workspace `{}`",
            path.display(),
            workspace_root.display()
        ));
    }
    Ok(resolved)
}

fn run_hooks(
    script: &ResolvedDrillScript,
    lifecycle: DrillHookLifecycle,
    report: &mut DrillReport,
    exit_code: &mut i32,
) -> Result<()> {
    let hooks = match lifecycle {
        DrillHookLifecycle::Setup => &script.setup,
        DrillHookLifecycle::Teardown => &script.teardown,
    };
    let mut first_error = None;
    for (index, hook) in hooks.iter().enumerate() {
        if first_error.is_some()
            && matches!(lifecycle, DrillHookLifecycle::Teardown)
            && !hook.always
        {
            continue;
        }
        let hook_report = execute_hook(hook, lifecycle);
        let id = format!("script-{}-{}", lifecycle_name(lifecycle), index + 1);
        let result = serde_json::to_value(&hook_report)?;
        match hook_exit_kind(&hook_report) {
            HookExitKind::Passed => {
                report.push_hook(script, hook_report.clone());
                report.push_phase(DrillPhase::passed(
                    id,
                    "commandHook",
                    Instant::now(),
                    format!("{} hook `{}` passed", lifecycle_name(lifecycle), hook.name),
                    Some(result),
                ));
            }
            HookExitKind::TimedOut => {
                apply_hook_exit(exit_code, lifecycle, HookExitKind::TimedOut);
                report.push_hook(script, hook_report.clone());
                report.push_phase(DrillPhase::failed(
                    id,
                    "commandHook",
                    Instant::now(),
                    format!(
                        "{} hook `{}` timed out after {}ms",
                        lifecycle_name(lifecycle),
                        hook.name,
                        hook.timeout_ms
                    ),
                    Some(result),
                ));
                let error = anyhow!(
                    "{} hook `{}` timed out",
                    lifecycle_name(lifecycle),
                    hook.name
                );
                if matches!(lifecycle, DrillHookLifecycle::Setup) {
                    return Err(error);
                }
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
            HookExitKind::Failed => {
                apply_hook_exit(exit_code, lifecycle, HookExitKind::Failed);
                report.push_hook(script, hook_report.clone());
                report.push_phase(DrillPhase::failed(
                    id,
                    "commandHook",
                    Instant::now(),
                    format!("{} hook `{}` failed", lifecycle_name(lifecycle), hook.name),
                    Some(result),
                ));
                let error = anyhow!("{} hook `{}` failed", lifecycle_name(lifecycle), hook.name);
                if matches!(lifecycle, DrillHookLifecycle::Setup) {
                    return Err(error);
                }
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }
    if let Some(error) = first_error {
        Err(error)
    } else {
        Ok(())
    }
}

fn run_teardown_hooks(script: &ResolvedDrillScript, report: &mut DrillReport, exit_code: &mut i32) {
    let _ = run_hooks(script, DrillHookLifecycle::Teardown, report, exit_code);
}

fn run_gates(
    script: &ResolvedDrillScript,
    app: &DiscoveredApp,
    child: Option<&mut Child>,
    report: &mut DrillReport,
    exit_code: &mut i32,
) -> Result<bool> {
    let gate_responses = if script.gates.iter().any(gate_accepts_terminal_response) {
        Some(spawn_gate_response_reader())
    } else {
        None
    };
    let mut child = child;
    for (index, gate) in script.gates.iter().enumerate() {
        let id = format!("script-gate-{}", index + 1);
        match execute_gate(gate, app, child.as_deref_mut(), gate_responses.as_ref())? {
            GateOutcome::Passed(gate_report) => {
                let result = serde_json::to_value(&gate_report)?;
                report.push_gate(script, gate_report.clone());
                report.push_phase(DrillPhase::passed(
                    id,
                    "humanGate",
                    Instant::now(),
                    format!(
                        "human gate `{}` satisfied by {}",
                        gate.name,
                        gate_satisfied_by_name(gate_report.satisfied_by)
                    ),
                    Some(result),
                ));
            }
            GateOutcome::Skipped(gate_report) => {
                report.push_gate(script, gate_report);
                report.push_phase(DrillPhase::skipped(
                    id,
                    "humanGate",
                    format!("human gate `{}` skipped by selected choice", gate.name),
                ));
            }
            GateOutcome::AppExited(code) => {
                if *exit_code == EXIT_PASSED {
                    *exit_code = EXIT_APP_EXITED;
                }
                report.command.exit_code = code;
                report.push_phase(DrillPhase::failed(
                    id,
                    "humanGate",
                    Instant::now(),
                    format!("wrapped command exited during human gate `{}`", gate.name),
                    Some(json!({ "exitCode": code })),
                ));
                return Ok(false);
            }
            GateOutcome::TimedOut(observed_text, observed_error) => {
                if *exit_code == EXIT_PASSED {
                    *exit_code = EXIT_TIMEOUT;
                }
                report.command.timed_out = true;
                report.push_phase(DrillPhase::failed(
                    id,
                    "humanGate",
                    Instant::now(),
                    format!(
                        "human gate `{}` timed out after {}ms",
                        gate.name, gate.timeout_ms
                    ),
                    Some(json!({
                        "selector": gate.selector.clone(),
                        "expectText": gate.expect_text.clone(),
                        "observedText": observed_text,
                        "observedError": observed_error,
                    })),
                ));
                return Ok(false);
            }
            GateOutcome::Failed(message, observed_text) => {
                if *exit_code == EXIT_PASSED {
                    *exit_code = EXIT_CHECK_FAILED;
                }
                report.push_phase(DrillPhase::failed(
                    id,
                    "humanGate",
                    Instant::now(),
                    message,
                    Some(json!({
                        "selector": gate.selector.clone(),
                        "expectText": gate.expect_text.clone(),
                        "observedText": observed_text,
                    })),
                ));
                return Ok(false);
            }
            GateOutcome::FailedChoice(gate_report) => {
                if *exit_code == EXIT_PASSED {
                    *exit_code = EXIT_CHECK_FAILED;
                }
                let result = serde_json::to_value(&gate_report)?;
                report.push_gate(script, gate_report);
                report.push_phase(DrillPhase::failed(
                    id,
                    "humanGate",
                    Instant::now(),
                    format!("human gate `{}` failed by selected choice", gate.name),
                    Some(result),
                ));
                return Ok(false);
            }
            GateOutcome::Aborted(gate_report) => {
                if *exit_code == EXIT_PASSED {
                    *exit_code = EXIT_CHECK_FAILED;
                }
                let result = serde_json::to_value(&gate_report)?;
                report.push_gate(script, gate_report);
                report.push_phase(DrillPhase::failed(
                    id,
                    "humanGate",
                    Instant::now(),
                    format!("human gate `{}` aborted by selected choice", gate.name),
                    Some(result),
                ));
                return Ok(false);
            }
            GateOutcome::Retry => {}
        }
    }
    Ok(true)
}

enum GateOutcome {
    Passed(DrillGateReport),
    Skipped(DrillGateReport),
    AppExited(Option<i32>),
    TimedOut(Option<String>, Option<String>),
    Failed(String, Option<String>),
    FailedChoice(DrillGateReport),
    Aborted(DrillGateReport),
    Retry,
}

fn execute_gate(
    gate: &ResolvedDrillGate,
    app: &DiscoveredApp,
    child: Option<&mut Child>,
    gate_responses: Option<&std::sync::mpsc::Receiver<String>>,
) -> Result<GateOutcome> {
    let started = Instant::now();
    let deadline = started + Duration::from_millis(gate.timeout_ms);
    let clipboard = prepare_gate_clipboard(gate)?;
    let gate_request = publish_gate_request(gate, app, &clipboard, started, deadline)?;
    print_gate_prompt(gate, gate_request.as_ref())?;
    let mut child = child;
    let mut observed_text = None;
    let mut observed_error = None;
    loop {
        if let Some(request) = &gate_request {
            if let Some(response) = read_gate_queue_response(request)? {
                let outcome = apply_gate_queue_response(
                    gate,
                    response,
                    request,
                    started,
                    observed_text.clone(),
                    clipboard.clone(),
                )?;
                if !matches!(outcome, GateOutcome::Retry) {
                    cleanup_gate_request(request);
                    return Ok(outcome);
                }
                remove_gate_response(request);
                print_gate_prompt(gate, gate_request.as_ref())?;
            }
        }
        match poll_gate_selector(gate, app) {
            Ok(Some(text)) => {
                observed_error = None;
                observed_text = Some(text.clone());
                if gate_text_matches(gate, &text) {
                    return Ok(GateOutcome::Passed(DrillGateReport {
                        name: gate.name.clone(),
                        instructions: gate.instructions.clone(),
                        selector: gate.selector.clone(),
                        expect_text: gate.expect_text.clone(),
                        timeout_ms: gate.timeout_ms,
                        manual_continue: gate.manual_continue,
                        duration_ms: started.elapsed().as_millis(),
                        satisfied_by: DrillGateSatisfiedBy::SelectorText,
                        observed_text,
                        selected_choice: None,
                        responder: None,
                        inputs: Vec::new(),
                        clipboard: clipboard.clone(),
                    }));
                }
            }
            Ok(None) => {}
            Err(error) if gate_accepts_terminal_response(gate) => {
                observed_error = Some(error.to_string());
            }
            Err(error) => return Ok(GateOutcome::Failed(error.to_string(), observed_text)),
        }
        if gate_accepts_terminal_response(gate) {
            let Some(receiver) = gate_responses else {
                continue;
            };
            match receiver.try_recv() {
                Ok(line) => {
                    if let Some(outcome) = apply_gate_response(
                        gate,
                        line,
                        receiver,
                        started,
                        deadline,
                        child.as_deref_mut(),
                        observed_text.clone(),
                        clipboard.clone(),
                    )? {
                        if let Some(request) = &gate_request {
                            cleanup_gate_request(request);
                        }
                        return Ok(outcome);
                    }
                    print_gate_prompt(gate, gate_request.as_ref())?;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {}
            }
        }
        if let Some(child) = child.as_deref_mut() {
            if let Some(status) = child.try_wait()? {
                if let Some(request) = &gate_request {
                    cleanup_gate_request(request);
                }
                return Ok(GateOutcome::AppExited(status.code()));
            }
        }
        if Instant::now() >= deadline {
            if let Some(request) = &gate_request {
                cleanup_gate_request(request);
            }
            return Ok(GateOutcome::TimedOut(observed_text, observed_error));
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn publish_gate_request(
    gate: &ResolvedDrillGate,
    app: &DiscoveredApp,
    clipboard: &Option<DrillGateClipboardReport>,
    started: Instant,
    deadline: Instant,
) -> Result<Option<PublishedGateRequest>> {
    if !gate_accepts_terminal_response(gate) {
        return Ok(None);
    }
    let db_path = PathBuf::from(&app.database_path);
    if !db_path.is_absolute() {
        return Ok(None);
    }
    let Some(session_dir) = db_path.parent() else {
        return Ok(None);
    };
    let request_root = session_dir.join("human-gates");
    let requests_dir = request_root.join("requests");
    let responses_dir = request_root.join("responses");
    fs::create_dir_all(&requests_dir).with_context(|| {
        format!(
            "failed to create human gate request directory {}",
            requests_dir.display()
        )
    })?;
    fs::create_dir_all(&responses_dir).with_context(|| {
        format!(
            "failed to create human gate response directory {}",
            responses_dir.display()
        )
    })?;

    let now = OffsetDateTime::now_utc().unix_timestamp_nanos();
    let run_id = format!("drill-{}-{}", std::process::id(), now);
    let gate_id = slugify_gate_id(&gate.name);
    let request_id = format!("{run_id}-{gate_id}-1");
    let nonce = format!("{}-{}", request_id, now.wrapping_mul(31));
    let request_path = requests_dir.join(format!("{request_id}.json"));
    let response_path = responses_dir.join(format!("{request_id}.json"));
    let request = json!({
        "schemaVersion": 1,
        "requestId": request_id,
        "runId": run_id,
        "gateId": gate_id,
        "attempt": 1,
        "nonce": nonce,
        "createdAtUnixNanos": now,
        "expiresAtUnixNanos": now + (deadline.saturating_duration_since(started).as_nanos() as i128),
        "session": {
            "sessionId": app.session_id,
            "instanceId": app.instance_id,
            "pid": app.pid,
            "databasePath": app.database_path,
        },
        "prompt": {
            "name": gate.name,
            "instructions": gate.instructions,
            "manualContinue": gate.manual_continue,
            "selector": gate.selector,
            "expectText": gate.expect_text,
            "timeoutMs": gate.timeout_ms,
        },
        "choices": gate.choices,
        "inputs": gate.inputs,
        "clipboard": clipboard,
    });
    write_json_atomic(&request_path, &request)?;
    Ok(Some(PublishedGateRequest {
        request_id,
        request_path: request_path.clone(),
        response_path: response_path.clone(),
        cleanup_paths: vec![request_path, response_path],
        nonce,
        run_id,
        gate_id,
        attempt: 1,
    }))
}

fn read_gate_queue_response(request: &PublishedGateRequest) -> Result<Option<GateQueueResponse>> {
    if !request.response_path.exists() {
        return Ok(None);
    }
    let value = fs::read_to_string(&request.response_path).with_context(|| {
        format!(
            "failed to read human gate response {}",
            request.response_path.display()
        )
    })?;
    let response: GateQueueResponse = serde_json::from_str(&value).with_context(|| {
        format!(
            "failed to parse human gate response {}",
            request.response_path.display()
        )
    })?;
    if response.schema_version != 1 {
        return Err(anyhow!(
            "human gate response `{}` uses unsupported schemaVersion {}",
            request.request_id,
            response.schema_version
        ));
    }
    if response.request_id != request.request_id
        || response.run_id != request.run_id
        || response.gate_id != request.gate_id
        || response.attempt != request.attempt
        || response.nonce != request.nonce
    {
        return Err(anyhow!(
            "human gate response `{}` did not match the pending request",
            request.request_id
        ));
    }
    Ok(Some(response))
}

fn apply_gate_queue_response(
    gate: &ResolvedDrillGate,
    response: GateQueueResponse,
    request: &PublishedGateRequest,
    started: Instant,
    observed_text: Option<String>,
    clipboard: Option<DrillGateClipboardReport>,
) -> Result<GateOutcome> {
    let choice_line = response
        .choice_id
        .as_deref()
        .unwrap_or(DEFAULT_MANUAL_CHOICE_ID);
    let Some(choice) = select_gate_choice(gate, choice_line)? else {
        return Err(anyhow!(
            "human gate response `{}` did not select a valid choice",
            request.request_id
        ));
    };
    if choice.outcome == DrillGateChoiceOutcome::Retry {
        return Ok(GateOutcome::Retry);
    }

    let mut inputs = Vec::new();
    for input in &gate.inputs {
        let value = response
            .inputs
            .iter()
            .find(|candidate| candidate.id == input.id)
            .map(|candidate| candidate.value.clone())
            .unwrap_or_default();
        if input.required && value.trim().is_empty() {
            return Err(anyhow!(
                "human gate response `{}` omitted required input `{}`",
                request.request_id,
                input.id
            ));
        }
        inputs.push(gate_input_report(input, value));
    }

    let selected_choice = if choice.id == DEFAULT_MANUAL_CHOICE_ID {
        None
    } else {
        Some(choice.clone())
    };
    let satisfied_by = if selected_choice.is_some() {
        DrillGateSatisfiedBy::Choice
    } else {
        DrillGateSatisfiedBy::ManualContinue
    };
    let report = gate_report(
        gate,
        started,
        satisfied_by,
        observed_text,
        selected_choice,
        inputs,
        clipboard,
        response.responder.or_else(|| Some("gateQueue".to_string())),
    );
    Ok(match choice.outcome {
        DrillGateChoiceOutcome::Continue => GateOutcome::Passed(report),
        DrillGateChoiceOutcome::Skip => GateOutcome::Skipped(report),
        DrillGateChoiceOutcome::Fail => GateOutcome::FailedChoice(report),
        DrillGateChoiceOutcome::Abort => GateOutcome::Aborted(report),
        DrillGateChoiceOutcome::Retry => unreachable!("retry is handled before reporting"),
    })
}

fn cleanup_gate_request(request: &PublishedGateRequest) {
    for path in &request.cleanup_paths {
        let _ = fs::remove_file(path);
    }
}

fn remove_gate_response(request: &PublishedGateRequest) {
    let _ = fs::remove_file(&request.response_path);
}

fn write_json_atomic(path: &Path, value: &Value) -> Result<()> {
    let tmp_path = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(value)?;
    fs::write(&tmp_path, text)
        .with_context(|| format!("failed to write temporary JSON {}", tmp_path.display()))?;
    fs::rename(&tmp_path, path)
        .with_context(|| format!("failed to atomically publish JSON {}", path.display()))?;
    Ok(())
}

fn slugify_gate_id(value: &str) -> String {
    let mut output = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
        } else if !output.ends_with('-') {
            output.push('-');
        }
    }
    let output = output.trim_matches('-').to_string();
    if output.is_empty() {
        "gate".to_string()
    } else {
        output
    }
}

pub(crate) fn pending_human_gate_requests() -> Result<Value> {
    let data_dir = resolve_data_dir(None)?;
    let sessions_dir = data_dir.join("sessions");
    let mut pending = Vec::new();
    if !sessions_dir.exists() {
        return Ok(json!([]));
    }
    for session in fs::read_dir(&sessions_dir).with_context(|| {
        format!(
            "failed to read sessions directory {}",
            sessions_dir.display()
        )
    })? {
        let session = session?;
        let requests_dir = session.path().join("human-gates").join("requests");
        if !requests_dir.exists() {
            continue;
        }
        for request_entry in fs::read_dir(&requests_dir).with_context(|| {
            format!(
                "failed to read human gate requests directory {}",
                requests_dir.display()
            )
        })? {
            let request_entry = request_entry?;
            let request_path = request_entry.path();
            if request_path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Some(request_id) = request_path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let response_path = session
                .path()
                .join("human-gates")
                .join("responses")
                .join(format!("{request_id}.json"));
            if response_path.exists() {
                continue;
            }
            let text = fs::read_to_string(&request_path).with_context(|| {
                format!(
                    "failed to read human gate request {}",
                    request_path.display()
                )
            })?;
            let mut request: Value = serde_json::from_str(&text).with_context(|| {
                format!(
                    "failed to parse human gate request {}",
                    request_path.display()
                )
            })?;
            if human_gate_request_expired(&request) {
                let _ = fs::remove_file(&request_path);
                continue;
            }
            if let Some(object) = request.as_object_mut() {
                object.insert(
                    "requestPath".to_string(),
                    Value::String(request_path.display().to_string()),
                );
                object.insert(
                    "responsePath".to_string(),
                    Value::String(response_path.display().to_string()),
                );
            }
            pending.push(request);
        }
    }
    pending.sort_by_key(|request| {
        request
            .get("createdAtUnixNanos")
            .and_then(Value::as_i64)
            .unwrap_or_default()
    });
    Ok(Value::Array(pending))
}

fn human_gate_request_expired(request: &Value) -> bool {
    let Some(expires_at) = request.get("expiresAtUnixNanos").and_then(Value::as_i64) else {
        return false;
    };
    i128::from(expires_at) <= OffsetDateTime::now_utc().unix_timestamp_nanos()
}

pub(crate) fn respond_human_gate(
    request_id: &str,
    choice_id: Option<String>,
    inputs: Value,
    responder: Option<String>,
) -> Result<Value> {
    let request_path = find_human_gate_request(request_id)?;
    let request_text = fs::read_to_string(&request_path).with_context(|| {
        format!(
            "failed to read human gate request {}",
            request_path.display()
        )
    })?;
    let request: Value = serde_json::from_str(&request_text).with_context(|| {
        format!(
            "failed to parse human gate request {}",
            request_path.display()
        )
    })?;
    let parent = request_path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| anyhow!("invalid human gate request path {}", request_path.display()))?;
    let response_path = parent.join("responses").join(format!("{request_id}.json"));
    fs::create_dir_all(
        response_path
            .parent()
            .ok_or_else(|| anyhow!("invalid response path {}", response_path.display()))?,
    )?;
    let response = json!({
        "schemaVersion": 1,
        "requestId": request_id,
        "runId": required_json_string(&request, "runId")?,
        "gateId": required_json_string(&request, "gateId")?,
        "attempt": required_json_i64(&request, "attempt")?,
        "nonce": required_json_string(&request, "nonce")?,
        "responder": responder.unwrap_or_else(|| "mcp".to_string()),
        "choiceId": choice_id,
        "inputs": normalize_gate_response_inputs(inputs)?,
    });
    write_json_atomic(&response_path, &response)?;
    Ok(json!({
        "requestId": request_id,
        "responsePath": response_path.display().to_string(),
        "status": "responded"
    }))
}

fn find_human_gate_request(request_id: &str) -> Result<PathBuf> {
    if request_id.contains(['\\', '/', ':']) {
        return Err(anyhow!(
            "requestId must be a human gate request id, not a path"
        ));
    }
    let data_dir = resolve_data_dir(None)?;
    let sessions_dir = data_dir.join("sessions");
    if !sessions_dir.exists() {
        return Err(anyhow!("no Auditaur sessions directory found"));
    }
    for session in fs::read_dir(&sessions_dir)? {
        let request_path = session?
            .path()
            .join("human-gates")
            .join("requests")
            .join(format!("{request_id}.json"));
        if request_path.exists() {
            return Ok(request_path);
        }
    }
    Err(anyhow!(
        "no pending human gate request matched `{request_id}`"
    ))
}

fn required_json_string(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| anyhow!("human gate request missing `{key}`"))
}

fn required_json_i64(value: &Value, key: &str) -> Result<i64> {
    value
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow!("human gate request missing `{key}`"))
}

fn normalize_gate_response_inputs(inputs: Value) -> Result<Value> {
    match inputs {
        Value::Null => Ok(json!([])),
        Value::Array(values) => Ok(Value::Array(values)),
        Value::Object(object) => {
            let values = object
                .into_iter()
                .map(|(id, value)| {
                    json!({
                        "id": id,
                        "value": value.as_str().map(ToString::to_string).unwrap_or_else(|| value.to_string())
                    })
                })
                .collect();
            Ok(Value::Array(values))
        }
        _ => Err(anyhow!("inputs must be an object, array, or null")),
    }
}

fn spawn_gate_response_reader() -> std::sync::mpsc::Receiver<String> {
    let (sender, receiver) = std::sync::mpsc::channel();
    thread::spawn(move || loop {
        let mut line = String::new();
        let Ok(bytes_read) = io::stdin().read_line(&mut line) else {
            break;
        };
        if bytes_read == 0 {
            break;
        }
        if sender.send(line.trim_end().to_string()).is_err() {
            break;
        }
    });
    receiver
}

fn gate_accepts_terminal_response(gate: &ResolvedDrillGate) -> bool {
    gate.manual_continue || !gate.choices.is_empty() || !gate.inputs.is_empty()
}

fn apply_gate_response(
    gate: &ResolvedDrillGate,
    line: String,
    receiver: &std::sync::mpsc::Receiver<String>,
    started: Instant,
    deadline: Instant,
    child: Option<&mut Child>,
    observed_text: Option<String>,
    clipboard: Option<DrillGateClipboardReport>,
) -> Result<Option<GateOutcome>> {
    let Some(choice) = select_gate_choice(gate, &line)? else {
        return Ok(None);
    };
    if choice.outcome == DrillGateChoiceOutcome::Retry {
        return Ok(None);
    }
    let inputs = match collect_gate_inputs(gate, receiver, deadline, child)? {
        GateInputOutcome::Collected(inputs) => inputs,
        GateInputOutcome::AppExited(code) => return Ok(Some(GateOutcome::AppExited(code))),
        GateInputOutcome::TimedOut => return Ok(Some(GateOutcome::TimedOut(observed_text, None))),
    };
    let selected_choice = if choice.id == DEFAULT_MANUAL_CHOICE_ID {
        None
    } else {
        Some(choice.clone())
    };
    let satisfied_by = if selected_choice.is_some() {
        DrillGateSatisfiedBy::Choice
    } else {
        DrillGateSatisfiedBy::ManualContinue
    };
    let report = gate_report(
        gate,
        started,
        satisfied_by,
        observed_text,
        selected_choice,
        inputs,
        clipboard,
        Some("terminal".to_string()),
    );
    Ok(Some(match choice.outcome {
        DrillGateChoiceOutcome::Continue => GateOutcome::Passed(report),
        DrillGateChoiceOutcome::Skip => GateOutcome::Skipped(report),
        DrillGateChoiceOutcome::Fail => GateOutcome::FailedChoice(report),
        DrillGateChoiceOutcome::Abort => GateOutcome::Aborted(report),
        DrillGateChoiceOutcome::Retry => unreachable!("retry is handled before input collection"),
    }))
}

const DEFAULT_MANUAL_CHOICE_ID: &str = "__manual_continue";

fn select_gate_choice(gate: &ResolvedDrillGate, line: &str) -> Result<Option<DrillGateChoice>> {
    let input = line.trim();
    if gate.choices.is_empty() {
        if gate.manual_continue || !gate.inputs.is_empty() {
            return Ok(Some(DrillGateChoice {
                id: DEFAULT_MANUAL_CHOICE_ID.to_string(),
                label: "Manual continue".to_string(),
                outcome: DrillGateChoiceOutcome::Continue,
            }));
        }
        return Ok(None);
    }
    if input.is_empty() && gate.manual_continue {
        if let Some(choice) = gate
            .choices
            .iter()
            .find(|choice| choice.outcome == DrillGateChoiceOutcome::Continue)
        {
            return Ok(Some(choice.clone()));
        }
    }
    if let Ok(index) = input.parse::<usize>() {
        if let Some(choice) = gate.choices.get(index.saturating_sub(1)) {
            return Ok(Some(choice.clone()));
        }
    }
    if let Some(choice) = gate.choices.iter().find(|choice| choice.id == input) {
        return Ok(Some(choice.clone()));
    }
    eprintln!("Unrecognized gate choice `{input}`. Enter a choice number or id.");
    Ok(None)
}

enum GateInputOutcome {
    Collected(Vec<DrillGateInputReport>),
    AppExited(Option<i32>),
    TimedOut,
}

fn collect_gate_inputs(
    gate: &ResolvedDrillGate,
    receiver: &std::sync::mpsc::Receiver<String>,
    deadline: Instant,
    child: Option<&mut Child>,
) -> Result<GateInputOutcome> {
    let mut child = child;
    let mut inputs = Vec::new();
    for input in &gate.inputs {
        loop {
            print_gate_input_prompt(input)?;
            match read_gate_response_line(receiver, deadline, child.as_deref_mut())? {
                GateLineOutcome::Line(value) if input.required && value.trim().is_empty() => {
                    eprintln!("Input `{}` is required.", input.id);
                    continue;
                }
                GateLineOutcome::Line(value) => {
                    inputs.push(gate_input_report(input, value));
                    break;
                }
                GateLineOutcome::AppExited(code) => return Ok(GateInputOutcome::AppExited(code)),
                GateLineOutcome::TimedOut => return Ok(GateInputOutcome::TimedOut),
            }
        }
    }
    Ok(GateInputOutcome::Collected(inputs))
}

enum GateLineOutcome {
    Line(String),
    AppExited(Option<i32>),
    TimedOut,
}

fn read_gate_response_line(
    receiver: &std::sync::mpsc::Receiver<String>,
    deadline: Instant,
    child: Option<&mut Child>,
) -> Result<GateLineOutcome> {
    let mut child = child;
    loop {
        match receiver.recv_timeout(Duration::from_millis(200)) {
            Ok(line) => return Ok(GateLineOutcome::Line(line)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Ok(GateLineOutcome::TimedOut)
            }
        }
        if let Some(child) = child.as_deref_mut() {
            if let Some(status) = child.try_wait()? {
                return Ok(GateLineOutcome::AppExited(status.code()));
            }
        }
        if Instant::now() >= deadline {
            return Ok(GateLineOutcome::TimedOut);
        }
    }
}

fn gate_input_report(input: &DrillGateInput, value: String) -> DrillGateInputReport {
    DrillGateInputReport {
        id: input.id.clone(),
        label: input.label.clone(),
        kind: input.kind,
        required: input.required,
        sensitive: input.sensitive,
        redacted: input.sensitive,
        value: if value.is_empty() {
            None
        } else if input.sensitive {
            Some("[REDACTED]".to_string())
        } else {
            Some(value)
        },
    }
}

fn gate_report(
    gate: &ResolvedDrillGate,
    started: Instant,
    satisfied_by: DrillGateSatisfiedBy,
    observed_text: Option<String>,
    selected_choice: Option<DrillGateChoice>,
    inputs: Vec<DrillGateInputReport>,
    clipboard: Option<DrillGateClipboardReport>,
    responder: Option<String>,
) -> DrillGateReport {
    DrillGateReport {
        name: gate.name.clone(),
        instructions: gate.instructions.clone(),
        selector: gate.selector.clone(),
        expect_text: gate.expect_text.clone(),
        timeout_ms: gate.timeout_ms,
        manual_continue: gate.manual_continue,
        duration_ms: started.elapsed().as_millis(),
        satisfied_by,
        observed_text,
        selected_choice,
        responder,
        inputs,
        clipboard,
    }
}

fn prepare_gate_clipboard(gate: &ResolvedDrillGate) -> Result<Option<DrillGateClipboardReport>> {
    let Some(clipboard) = &gate.clipboard else {
        return Ok(None);
    };
    let (status, error) = match clipboard.copy {
        DrillGateClipboardCopyMode::Attempt => match copy_to_clipboard(&clipboard.value) {
            Ok(()) => (DrillGateClipboardStatus::Copied, None),
            Err(error) => (DrillGateClipboardStatus::Failed, Some(error.to_string())),
        },
        DrillGateClipboardCopyMode::ManualOnly => (DrillGateClipboardStatus::ManualOnly, None),
        DrillGateClipboardCopyMode::Disabled => (DrillGateClipboardStatus::Disabled, None),
    };
    print_clipboard_status(clipboard, status, error.as_deref())?;
    Ok(Some(DrillGateClipboardReport {
        label: clipboard.label.clone(),
        sensitive: clipboard.sensitive,
        redacted: clipboard.sensitive,
        value: if clipboard.sensitive {
            Some("[REDACTED]".to_string())
        } else {
            Some(clipboard.value.clone())
        },
        copy: clipboard.copy,
        status,
        error,
    }))
}

fn print_clipboard_status(
    clipboard: &DrillGateClipboard,
    status: DrillGateClipboardStatus,
    error: Option<&str>,
) -> Result<()> {
    let mut stderr = io::stderr().lock();
    match status {
        DrillGateClipboardStatus::Copied => {
            writeln!(
                stderr,
                "{} copied to clipboard. Paste it into the external prompt.",
                clipboard.label
            )?;
        }
        DrillGateClipboardStatus::Failed => {
            writeln!(
                stderr,
                "Could not copy {} to clipboard: {}",
                clipboard.label,
                error.unwrap_or("unknown clipboard error")
            )?;
            print_manual_clipboard_value(&mut stderr, clipboard)?;
        }
        DrillGateClipboardStatus::ManualOnly => {
            writeln!(stderr, "{} is available for manual copy.", clipboard.label)?;
            print_manual_clipboard_value(&mut stderr, clipboard)?;
        }
        DrillGateClipboardStatus::Disabled => {
            writeln!(
                stderr,
                "{} clipboard copy is disabled for this gate.",
                clipboard.label
            )?;
            print_manual_clipboard_value(&mut stderr, clipboard)?;
        }
    }
    stderr.flush()?;
    Ok(())
}

fn print_manual_clipboard_value(
    stderr: &mut impl Write,
    clipboard: &DrillGateClipboard,
) -> Result<()> {
    if clipboard.sensitive {
        writeln!(
            stderr,
            "{} is marked sensitive, so Auditaur will not print it for manual copy.",
            clipboard.label
        )?;
    } else {
        writeln!(stderr, "{}: {}", clipboard.label, clipboard.value)?;
    }
    Ok(())
}

fn copy_to_clipboard(value: &str) -> Result<()> {
    #[cfg(windows)]
    {
        return copy_to_clipboard_command(
            "powershell",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Set-Clipboard -Value $input",
            ],
            value,
        );
    }

    #[cfg(target_os = "macos")]
    {
        return copy_to_clipboard_command("pbcopy", &[], value);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut last_error = None;
        for (program, args) in [
            ("wl-copy", Vec::<&str>::new()),
            ("xclip", vec!["-selection", "clipboard"]),
            ("xsel", vec!["--clipboard", "--input"]),
        ] {
            match copy_to_clipboard_command(program, &args, value) {
                Ok(()) => return Ok(()),
                Err(error) => last_error = Some(error),
            }
        }
        return Err(last_error.unwrap_or_else(|| anyhow!("no clipboard command was attempted")));
    }

    #[cfg(not(any(windows, unix)))]
    {
        let _ = value;
        Err(anyhow!("clipboard copy is not supported on this platform"))
    }
}

fn copy_to_clipboard_command(program: &str, args: &[&str], value: &str) -> Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to start clipboard command `{program}`"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(value.as_bytes())
            .context("failed to write clipboard value")?;
    }
    let output = child
        .wait_with_output()
        .context("failed waiting for clipboard command")?;
    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "clipboard command `{program}` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn print_gate_prompt(
    gate: &ResolvedDrillGate,
    gate_request: Option<&PublishedGateRequest>,
) -> Result<()> {
    let mut stderr = io::stderr().lock();
    writeln!(stderr)?;
    writeln!(stderr, "Auditaur human gate: {}", gate.name)?;
    writeln!(stderr, "{}", gate.instructions)?;
    if let Some(request) = gate_request {
        writeln!(
            stderr,
            "Agent gate request: {}",
            request.request_path.display()
        )?;
    }
    if let Some(selector) = &gate.selector {
        if let Some(expected) = &gate.expect_text {
            writeln!(
                stderr,
                "Waiting for selector `{selector}` to contain `{expected}`."
            )?;
        } else {
            writeln!(stderr, "Waiting for selector `{selector}` to return text.")?;
        }
    }
    if gate.manual_continue {
        if gate.choices.is_empty() {
            writeln!(stderr, "Press ENTER here to continue manually.")?;
        } else {
            writeln!(
                stderr,
                "Press ENTER to select the first continue choice, or enter a choice below."
            )?;
        }
    } else if gate.choices.is_empty() && !gate.inputs.is_empty() {
        writeln!(
            stderr,
            "Press ENTER here to provide gate input and continue."
        )?;
    }
    if !gate.inputs.is_empty() {
        writeln!(
            stderr,
            "This gate will ask for {} input value(s) before continuing from a manual response.",
            gate.inputs.len()
        )?;
    }
    if !gate.choices.is_empty() {
        writeln!(stderr, "Choices:")?;
        for (index, choice) in gate.choices.iter().enumerate() {
            writeln!(
                stderr,
                "  {}. {} ({}, outcome: {})",
                index + 1,
                choice.label,
                choice.id,
                choice_outcome_name(choice.outcome)
            )?;
        }
    }
    stderr.flush()?;
    Ok(())
}

fn print_gate_input_prompt(input: &DrillGateInput) -> Result<()> {
    let mut stderr = io::stderr().lock();
    let required = if input.required {
        " required"
    } else {
        " optional"
    };
    let sensitivity = if input.sensitive {
        " sensitive, redacted"
    } else {
        " recorded"
    };
    let kind = match input.kind {
        DrillGateInputKind::Text => "text",
        DrillGateInputKind::MultilineText => "multiline text",
    };
    writeln!(stderr, "{} ({kind},{required},{sensitivity}):", input.label)?;
    stderr.flush()?;
    Ok(())
}

fn choice_outcome_name(outcome: DrillGateChoiceOutcome) -> &'static str {
    match outcome {
        DrillGateChoiceOutcome::Continue => "continue",
        DrillGateChoiceOutcome::Retry => "retry",
        DrillGateChoiceOutcome::Skip => "skip",
        DrillGateChoiceOutcome::Fail => "fail",
        DrillGateChoiceOutcome::Abort => "abort",
    }
}

fn poll_gate_selector(gate: &ResolvedDrillGate, app: &DiscoveredApp) -> Result<Option<String>> {
    let Some(selector) = &gate.selector else {
        return Ok(None);
    };
    let value = drive::text_json_value(
        drive_selector(app),
        drive::SelectorActionOptions {
            selector: selector.clone(),
            target_id: Some("auditaur-bridge".to_string()),
            test_id: Some("auditaur-drill".to_string()),
            step_id: Some(format!("human-gate-{}", gate.name)),
            visible_only: true,
            json: true,
        },
    )?;
    Ok(value
        .get("payload")
        .and_then(|payload| payload.get("text"))
        .and_then(Value::as_str)
        .map(ToString::to_string))
}

fn gate_text_matches(gate: &ResolvedDrillGate, text: &str) -> bool {
    match &gate.expect_text {
        Some(expected) => text.contains(expected),
        None => !text.is_empty(),
    }
}

fn gate_satisfied_by_name(satisfied_by: DrillGateSatisfiedBy) -> &'static str {
    match satisfied_by {
        DrillGateSatisfiedBy::SelectorText => "selector text",
        DrillGateSatisfiedBy::ManualContinue => "manual continue",
        DrillGateSatisfiedBy::Choice => "choice",
    }
}

fn execute_hook(hook: &ResolvedDrillHook, lifecycle: DrillHookLifecycle) -> DrillHookReport {
    let started = Instant::now();
    let timeout = Duration::from_millis(hook.timeout_ms);
    let mut command = Command::new(&hook.run);
    command
        .args(&hook.args)
        .current_dir(&hook.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);

    match command.spawn() {
        Ok(child) => wait_for_hook_output(hook, lifecycle, started, timeout, child),
        Err(error) => hook_report_from_error(hook, lifecycle, started, error),
    }
}

fn wait_for_hook_output(
    hook: &ResolvedDrillHook,
    lifecycle: DrillHookLifecycle,
    started: Instant,
    timeout: Duration,
    mut child: Child,
) -> DrillHookReport {
    let stdout = child.stdout.take().map(read_stream_thread);
    let stderr = child.stderr.take().map(read_stream_thread);
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break exit_code(status),
            Ok(None) if started.elapsed() >= timeout => {
                timed_out = true;
                let _ = cleanup_process_tree(child.id());
                break child.wait().ok().and_then(exit_code);
            }
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(error) => return hook_report_from_error(hook, lifecycle, started, error),
        }
    };

    hook_report(
        hook,
        lifecycle,
        started,
        status,
        timed_out,
        join_stream_thread(stdout),
        join_stream_thread(stderr),
    )
}

fn read_stream_thread<T>(mut stream: T) -> thread::JoinHandle<String>
where
    T: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0_u8; 8192];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    if output.len() < HOOK_OUTPUT_MAX_BYTES {
                        let remaining = HOOK_OUTPUT_MAX_BYTES - output.len();
                        output.extend_from_slice(&buffer[..count.min(remaining)]);
                    }
                }
                Err(_) => break,
            }
        }
        bounded_text(&String::from_utf8_lossy(&output), HOOK_OUTPUT_MAX_CHARS)
    })
}

fn join_stream_thread(handle: Option<thread::JoinHandle<String>>) -> String {
    handle
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default()
}

fn hook_report_from_error(
    hook: &ResolvedDrillHook,
    lifecycle: DrillHookLifecycle,
    started: Instant,
    error: std::io::Error,
) -> DrillHookReport {
    hook_report(
        hook,
        lifecycle,
        started,
        None,
        false,
        "",
        format!("failed to run hook command: {error}"),
    )
}

fn hook_report(
    hook: &ResolvedDrillHook,
    lifecycle: DrillHookLifecycle,
    started: Instant,
    exit_code: Option<i32>,
    timed_out: bool,
    stdout: impl AsRef<str>,
    stderr: impl AsRef<str>,
) -> DrillHookReport {
    DrillHookReport {
        name: hook.name.clone(),
        lifecycle,
        command: hook.run.clone(),
        args: hook.args.clone(),
        cwd: hook.cwd.to_string_lossy().to_string(),
        timeout_ms: hook.timeout_ms,
        always: hook.always,
        exit_code,
        duration_ms: started.elapsed().as_millis(),
        timed_out,
        stdout: bounded_text(stdout.as_ref(), HOOK_OUTPUT_MAX_CHARS),
        stderr: bounded_text(stderr.as_ref(), HOOK_OUTPUT_MAX_CHARS),
    }
}

fn hook_exit_kind(report: &DrillHookReport) -> HookExitKind {
    if report.timed_out {
        HookExitKind::TimedOut
    } else if report.exit_code == Some(0) {
        HookExitKind::Passed
    } else {
        HookExitKind::Failed
    }
}

fn apply_hook_exit(exit_code: &mut i32, lifecycle: DrillHookLifecycle, hook_exit: HookExitKind) {
    if *exit_code != EXIT_PASSED {
        return;
    }
    *exit_code = match (lifecycle, hook_exit) {
        (_, HookExitKind::TimedOut) => EXIT_TIMEOUT,
        (DrillHookLifecycle::Setup, HookExitKind::Failed) => EXIT_CHECK_FAILED,
        (DrillHookLifecycle::Teardown, HookExitKind::Failed) => EXIT_CLEANUP_FAILED,
        (_, HookExitKind::Passed) => EXIT_PASSED,
    };
}

fn exit_code(status: ExitStatus) -> Option<i32> {
    status.code()
}

fn default_hook_timeout_ms() -> u64 {
    DEFAULT_HOOK_TIMEOUT_MS
}

fn default_gate_timeout_ms() -> u64 {
    300_000
}

fn default_manual_continue() -> bool {
    true
}

fn default_gate_choice_outcome() -> DrillGateChoiceOutcome {
    DrillGateChoiceOutcome::Continue
}

fn default_clipboard_copy_mode() -> DrillGateClipboardCopyMode {
    DrillGateClipboardCopyMode::Attempt
}

fn default_sensitive_input() -> bool {
    true
}

fn default_hook_cwd() -> PathBuf {
    PathBuf::from(".")
}

fn lifecycle_name(lifecycle: DrillHookLifecycle) -> &'static str {
    match lifecycle {
        DrillHookLifecycle::Setup => "setup",
        DrillHookLifecycle::Teardown => "teardown",
    }
}

fn run_post_readiness_phases(
    options: &DrillRunOptions,
    app: &DiscoveredApp,
    report: &mut DrillReport,
    exit_code: &mut i32,
) {
    run_phase(report, exit_code, "drive-inspect", "driveInspect", || {
        drive::inspect_json_value(drive_selector(app))
    });

    if options.require_drive_bridge || options.selector.is_some() || options.expect_text.is_some() {
        run_phase_result(
            report,
            exit_code,
            "drive-bridge-responsive",
            "drivePing",
            || {
                let value = drive::ping_json_value(drive_selector(app))?;
                let ok = value.get("ok").and_then(Value::as_bool).unwrap_or(false);
                let status = value
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string();
                Ok((
                    value,
                    ok,
                    if ok {
                        "drive bridge responded to ping".to_string()
                    } else {
                        format!("drive bridge action responsiveness failed: {status}")
                    },
                ))
            },
        );
    } else {
        report.push_phase(DrillPhase::skipped(
            "drive-bridge-responsive",
            "driveEvaluate",
            "drive bridge responsiveness was not required",
        ));
    }

    if options.selector.is_some() || options.expect_text.is_some() {
        let selector = options
            .selector
            .clone()
            .unwrap_or_else(|| "body".to_string());
        let expected = options.expect_text.clone();
        run_phase(report, exit_code, "drive-text", "driveText", || {
            let value = drive::text_json_value(
                drive_selector(app),
                drive::SelectorActionOptions {
                    selector: selector.clone(),
                    target_id: Some("auditaur-bridge".to_string()),
                    test_id: Some("auditaur-drill".to_string()),
                    step_id: Some("drive-text".to_string()),
                    visible_only: true,
                    json: true,
                },
            )?;
            if let Some(expected) = &expected {
                let text = value
                    .get("payload")
                    .and_then(|payload| payload.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !text.contains(expected) {
                    return Err(anyhow!(
                        "selector `{selector}` text did not contain expected text `{expected}`"
                    ));
                }
            }
            Ok(value)
        });
    } else {
        report.push_phase(DrillPhase::skipped(
            "drive-text",
            "driveText",
            "no selector or expected text was supplied",
        ));
    }

    run_telemetry_phases(options, app, report, exit_code);
}

fn run_telemetry_phases(
    _options: &DrillRunOptions,
    app: &DiscoveredApp,
    report: &mut DrillReport,
    exit_code: &mut i32,
) {
    let db = PathBuf::from(&app.database_path);
    run_phase_result(report, exit_code, "errors", "telemetryErrors", || {
        let store = read::open_validated_store(&db)?;
        let errors = store.list_frontend_errors(&FrontendErrorQuery {
            session_id: Some(app.session_id.clone()),
            trace_id: None,
            limit: Some(200),
        })?;
        let count = errors.len();
        Ok((
            json!({ "count": count, "items": errors }),
            count == 0,
            if count == 0 {
                "no frontend errors found".to_string()
            } else {
                format!("found {count} frontend error(s)")
            },
        ))
    });

    run_phase_result(report, exit_code, "failed-ipc", "telemetryIpc", || {
        let store = read::open_validated_store(&db)?;
        let calls = store.list_tauri_ipc_calls(&TauriIpcQuery {
            session_id: Some(app.session_id.clone()),
            trace_id: None,
            limit: Some(200),
        })?;
        let failed = calls
            .into_iter()
            .filter(read::is_failed_ipc)
            .collect::<Vec<_>>();
        let count = failed.len();
        Ok((
            json!({ "count": count, "items": failed }),
            count == 0,
            if count == 0 {
                "no failed IPC calls found".to_string()
            } else {
                format!("found {count} failed IPC call(s)")
            },
        ))
    });

    run_phase(report, exit_code, "explain", "telemetryExplain", || {
        polish::explain_json_value(
            &Some(db.clone()),
            Some(app.session_id.clone()),
            None,
            None,
            200,
        )
    });
}

fn run_phase(
    report: &mut DrillReport,
    exit_code: &mut i32,
    id: &str,
    kind: &str,
    phase: impl FnOnce() -> Result<Value>,
) {
    let started = Instant::now();
    match phase() {
        Ok(value) => report.push_phase(DrillPhase::passed(
            id,
            kind,
            started,
            format!("{id} passed"),
            Some(value),
        )),
        Err(error) => {
            if *exit_code == EXIT_PASSED {
                *exit_code = EXIT_CHECK_FAILED;
            }
            report.push_phase(DrillPhase::failed(
                id,
                kind,
                started,
                error.to_string(),
                None,
            ));
        }
    }
}

fn run_phase_result(
    report: &mut DrillReport,
    exit_code: &mut i32,
    id: &str,
    kind: &str,
    phase: impl FnOnce() -> Result<(Value, bool, String)>,
) {
    let started = Instant::now();
    match phase() {
        Ok((value, true, message)) => {
            report.push_phase(DrillPhase::passed(id, kind, started, message, Some(value)))
        }
        Ok((value, false, message)) => {
            if *exit_code == EXIT_PASSED {
                *exit_code = EXIT_CHECK_FAILED;
            }
            report.push_phase(DrillPhase::failed(id, kind, started, message, Some(value)));
        }
        Err(error) => {
            if *exit_code == EXIT_PASSED {
                *exit_code = EXIT_CHECK_FAILED;
            }
            report.push_phase(DrillPhase::failed(
                id,
                kind,
                started,
                error.to_string(),
                None,
            ));
        }
    }
}

fn wait_for_spawned_session(
    app_name: &str,
    started_at: OffsetDateTime,
    preexisting: &[DiscoveredApp],
    deadline: Instant,
    interval: Duration,
    child: Option<&mut Child>,
    report: &mut DrillReport,
) -> Result<Option<DiscoveredApp>> {
    let started = Instant::now();
    let preexisting_ids = preexisting
        .iter()
        .map(|app| app.instance_id.as_str())
        .collect::<HashSet<_>>();
    let mut child = child;
    loop {
        if let Some(app) = find_spawned_session(app_name, started_at, &preexisting_ids)? {
            report.push_phase(DrillPhase::passed(
                "spawn-owned-session",
                "discovery",
                started,
                format!("pinned spawned Auditaur session {}", app.session_id),
                Some(json!({
                    "sessionId": app.session_id,
                    "instanceId": app.instance_id,
                    "pid": app.pid,
                    "databasePath": app.database_path,
                    "pinnedBy": "newDiscoveryAfterSpawn"
                })),
            ));
            return Ok(Some(app));
        }
        if let Some(child) = child.as_deref_mut() {
            if let Some(status) = child.try_wait()? {
                report.command.exit_code = status.code();
                report.push_phase(DrillPhase::failed(
                    "spawn-owned-session",
                    "discovery",
                    started,
                    "wrapped command exited before a new Auditaur session was discovered",
                    Some(json!({ "exitCode": status.code() })),
                ));
                return Ok(None);
            }
        }
        if Instant::now() >= deadline {
            report.command.timed_out = true;
            report.push_phase(DrillPhase::failed(
                "spawn-owned-session",
                "discovery",
                started,
                "timed out waiting for a new Auditaur session after spawn; preexisting sessions were ignored",
                Some(json!({
                    "preexistingMatchingSessions": preexisting.len(),
                    "preexistingMatchingSessionIds": preexisting.iter().map(|app| app.session_id.clone()).collect::<Vec<_>>(),
                    "preexistingMatchingInstanceIds": preexisting.iter().map(|app| app.instance_id.clone()).collect::<Vec<_>>()
                })),
            ));
            return Ok(None);
        }
        thread::sleep(interval);
    }
}

enum WaitOutcome {
    Ready(debug::DebugStatus),
    AppExited(Option<i32>),
    TimedOut(Option<debug::DebugStatus>),
}

fn wait_for_debug_ready(
    options: &DrillRunOptions,
    app: &DiscoveredApp,
    deadline: Instant,
    interval: Duration,
    child: Option<&mut Child>,
    _report: &mut DrillReport,
) -> Result<WaitOutcome> {
    let selector = debug::DebugSelector {
        db: Some(PathBuf::from(&app.database_path)),
        app: None,
        session_id: Some(app.session_id.clone()),
        instance_id: Some(app.instance_id.clone()),
        pid: Some(app.pid),
        latest: false,
        active: false,
        cdp_port: None,
        require_frontend: options.require_frontend,
        require_drive_bridge: options.require_drive_bridge,
    };
    let mut child = child;
    loop {
        let status = debug::snapshot(&selector)?;
        if status.ready {
            return Ok(WaitOutcome::Ready(status));
        }
        if let Some(child) = child.as_deref_mut() {
            if let Some(status) = child.try_wait()? {
                return Ok(WaitOutcome::AppExited(status.code()));
            }
        }
        if Instant::now() >= deadline {
            return Ok(WaitOutcome::TimedOut(Some(status)));
        }
        thread::sleep(interval);
    }
}

fn spawn_command(command: &[String], environment: &[(String, String)]) -> Result<Child> {
    if command.is_empty() {
        return Err(anyhow!("drill run requires a command after `--`"));
    }
    let mut child_command = Command::new(&command[0]);
    child_command
        .args(&command[1..])
        .envs(environment.iter().map(|(key, value)| (key, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    child_command.creation_flags(0x0800_0000);
    #[cfg(not(windows))]
    child_command.process_group(0);
    child_command
        .spawn()
        .with_context(|| format!("failed to start `{}`", command.join(" ")))
}

fn matching_sessions(app_name: &str) -> Result<Vec<DiscoveredApp>> {
    let mut apps = discovery::list_apps()?
        .into_iter()
        .filter(|app| app_matches(app, app_name))
        .collect::<Vec<_>>();
    apps.sort_by(|left, right| right.last_heartbeat_at.cmp(&left.last_heartbeat_at));
    Ok(apps)
}

fn find_spawned_session(
    app_name: &str,
    started_at: OffsetDateTime,
    preexisting_ids: &HashSet<&str>,
) -> Result<Option<DiscoveredApp>> {
    let mut candidates = discovery::list_apps()?
        .into_iter()
        .filter(|app| app_matches(app, app_name))
        .filter(|app| app.status == DiscoveryStatus::Active)
        .filter(|app| !preexisting_ids.contains(app.instance_id.as_str()))
        .filter(|app| app_started_or_heartbeat_after(app, started_at))
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.last_heartbeat_at.cmp(&left.last_heartbeat_at));
    Ok(candidates.into_iter().next())
}

fn app_started_or_heartbeat_after(app: &DiscoveredApp, started_at: OffsetDateTime) -> bool {
    let cutoff = started_at - time::Duration::seconds(2);
    parse_rfc3339(&app.started_at).is_some_and(|time| time >= cutoff)
        || parse_rfc3339(&app.last_heartbeat_at).is_some_and(|time| time >= cutoff)
}

fn app_matches(app: &DiscoveredApp, needle: &str) -> bool {
    let needle = needle.to_ascii_lowercase();
    app.service_name.to_ascii_lowercase().contains(&needle)
        || app
            .app_identifier
            .as_deref()
            .is_some_and(|identifier| identifier.to_ascii_lowercase().contains(&needle))
        || app.session_id.to_ascii_lowercase().contains(&needle)
}

fn parse_rfc3339(value: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).ok()
}

fn drive_selector(app: &DiscoveredApp) -> drive::DriveAppSelector {
    drive::DriveAppSelector {
        app: None,
        session_id: Some(app.session_id.clone()),
        instance_id: Some(app.instance_id.clone()),
        pid: Some(app.pid),
        latest: false,
        active: false,
    }
}

fn cleanup_child_tree(child: &mut Child) -> Result<String> {
    if let Some(status) = child.try_wait()? {
        return Ok(format!(
            "wrapped command already exited with status {status}"
        ));
    }
    let pid = child.id();
    cleanup_process_tree(pid)?;
    let _ = child.wait();
    Ok(format!("stopped process tree rooted at pid {pid}"))
}

#[cfg(windows)]
fn cleanup_process_tree(pid: u32) -> Result<()> {
    let output = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .context("failed to invoke taskkill")?;
    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "taskkill failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(not(windows))]
fn cleanup_process_tree(pid: u32) -> Result<()> {
    let process_group = format!("-{pid}");
    let status = Command::new("kill")
        .args(["-TERM", &process_group])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .context("failed to invoke kill")?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("kill failed for process group {pid}"))
    }
}

fn finish(mut report: DrillReport, options: &DrillRunOptions, exit_code: i32) -> Result<i32> {
    report.exit_code = exit_code;
    report.status = status_for_exit_code(exit_code);
    report.summary = summarize_phases(&report.phases);
    report.generated_at_unix_nanos = read::current_time_unix_nanos();
    if let Some(parent) = options
        .report
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    report.artifacts.push(DrillArtifact {
        kind: "json".to_string(),
        path: options.report.to_string_lossy().to_string(),
        redacted: true,
    });
    let value = redacted_report_value(&report)?;
    fs::write(&options.report, serde_json::to_vec_pretty(&value)?)?;
    if options.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!(
            "Auditaur drill {:?}: {} passed, {} failed, {} skipped. Report: {}",
            report.status,
            report.summary.passed,
            report.summary.failed,
            report.summary.skipped,
            options.report.display()
        );
    }
    Ok(exit_code)
}

fn redacted_report_value(report: &DrillReport) -> Result<Value> {
    let value = serde_json::to_value(report)?;
    let result = redact_json(&value, &[]);
    let mut value = result.value;
    if let Some(redaction) = value.get_mut("redaction").and_then(Value::as_object_mut) {
        redaction.insert("redacted".to_string(), Value::Bool(result.redacted));
    }
    Ok(value)
}

fn summarize_phases(phases: &[DrillPhase]) -> DrillSummary {
    let mut summary = DrillSummary::default();
    for phase in phases {
        match phase.status {
            DrillPhaseStatus::Passed => summary.passed += 1,
            DrillPhaseStatus::Failed => summary.failed += 1,
            DrillPhaseStatus::Skipped => summary.skipped += 1,
        }
    }
    summary
}

fn status_for_exit_code(exit_code: i32) -> DrillStatus {
    match exit_code {
        EXIT_PASSED => DrillStatus::Passed,
        EXIT_CHECK_FAILED | EXIT_APP_EXITED | EXIT_CLEANUP_FAILED => DrillStatus::Failed,
        EXIT_TIMEOUT => DrillStatus::TimedOut,
        _ => DrillStatus::Error,
    }
}

impl DrillReport {
    fn new(options: &DrillRunOptions, preexisting: &[DiscoveredApp]) -> Self {
        Self {
            schema_version: 1,
            generated_at_unix_nanos: read::current_time_unix_nanos(),
            status: DrillStatus::Error,
            exit_code: EXIT_RUNNER_ERROR,
            app: DrillApp {
                service_name: options.app.clone(),
                session_id: None,
                instance_id: None,
                pid: None,
                database_path: None,
                started_at: None,
                pinned_by: None,
                preexisting_matching_sessions: preexisting
                    .iter()
                    .map(|app| app.session_id.clone())
                    .collect(),
            },
            command: DrillCommandReport {
                argv: options.command.clone(),
                root_pid: None,
                exit_code: None,
                running_at_readiness: None,
                timed_out: false,
            },
            script: options.script.as_ref().map(|path| DrillScriptReport {
                path: path.to_string_lossy().to_string(),
                workspace_root: String::new(),
                setup: Vec::new(),
                gates: Vec::new(),
                teardown: Vec::new(),
            }),
            options: DrillOptionsReport {
                require_frontend: options.require_frontend,
                require_drive_bridge: options.require_drive_bridge,
                timeout_seconds: options.timeout_seconds,
                selector: options.selector.clone(),
                expect_text: options.expect_text.clone(),
                script: options
                    .script
                    .as_ref()
                    .map(|path| path.to_string_lossy().to_string()),
            },
            phases: Vec::new(),
            summary: DrillSummary::default(),
            artifacts: Vec::new(),
            redaction: DrillRedaction {
                defaults: true,
                extra_keys: Vec::new(),
                redacted: false,
            },
        }
    }

    fn pin_app(&mut self, app: &DiscoveredApp) {
        self.app.session_id = Some(app.session_id.clone());
        self.app.instance_id = Some(app.instance_id.clone());
        self.app.pid = Some(app.pid);
        self.app.database_path = Some(app.database_path.clone());
        self.app.started_at = Some(app.started_at.clone());
        self.app.pinned_by = Some("newDiscoveryAfterSpawn".to_string());
    }

    fn push_phase(&mut self, phase: DrillPhase) {
        self.phases.push(phase);
    }

    fn set_script(&mut self, script: &ResolvedDrillScript) {
        self.script = Some(DrillScriptReport {
            path: script.path.to_string_lossy().to_string(),
            workspace_root: script.workspace_root.to_string_lossy().to_string(),
            setup: Vec::new(),
            gates: Vec::new(),
            teardown: Vec::new(),
        });
    }

    fn push_hook(&mut self, script: &ResolvedDrillScript, hook: DrillHookReport) {
        let script_report = self.script.get_or_insert_with(|| DrillScriptReport {
            path: script.path.to_string_lossy().to_string(),
            workspace_root: script.workspace_root.to_string_lossy().to_string(),
            setup: Vec::new(),
            gates: Vec::new(),
            teardown: Vec::new(),
        });
        script_report.path = script.path.to_string_lossy().to_string();
        script_report.workspace_root = script.workspace_root.to_string_lossy().to_string();
        match hook.lifecycle {
            DrillHookLifecycle::Setup => script_report.setup.push(hook),
            DrillHookLifecycle::Teardown => script_report.teardown.push(hook),
        }
    }

    fn push_gate(&mut self, script: &ResolvedDrillScript, gate: DrillGateReport) {
        let script_report = self.script.get_or_insert_with(|| DrillScriptReport {
            path: script.path.to_string_lossy().to_string(),
            workspace_root: script.workspace_root.to_string_lossy().to_string(),
            setup: Vec::new(),
            gates: Vec::new(),
            teardown: Vec::new(),
        });
        script_report.path = script.path.to_string_lossy().to_string();
        script_report.workspace_root = script.workspace_root.to_string_lossy().to_string();
        script_report.gates.push(gate);
    }
}

impl DrillPhase {
    fn passed(
        id: impl Into<String>,
        kind: impl Into<String>,
        started: Instant,
        message: impl Into<String>,
        result: Option<Value>,
    ) -> Self {
        Self {
            id: id.into(),
            kind: kind.into(),
            status: DrillPhaseStatus::Passed,
            duration_ms: started.elapsed().as_millis(),
            message: message.into(),
            result,
        }
    }

    fn failed(
        id: impl Into<String>,
        kind: impl Into<String>,
        started: Instant,
        message: impl Into<String>,
        result: Option<Value>,
    ) -> Self {
        Self {
            id: id.into(),
            kind: kind.into(),
            status: DrillPhaseStatus::Failed,
            duration_ms: started.elapsed().as_millis(),
            message: message.into(),
            result,
        }
    }

    fn skipped(id: impl Into<String>, kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind: kind.into(),
            status: DrillPhaseStatus::Skipped,
            duration_ms: 0,
            message: message.into(),
            result: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn exit_codes_map_to_status() {
        assert_eq!(status_for_exit_code(EXIT_PASSED), DrillStatus::Passed);
        assert_eq!(status_for_exit_code(EXIT_CHECK_FAILED), DrillStatus::Failed);
        assert_eq!(status_for_exit_code(EXIT_TIMEOUT), DrillStatus::TimedOut);
        assert_eq!(status_for_exit_code(EXIT_RUNNER_ERROR), DrillStatus::Error);
    }

    #[test]
    fn summarizes_phase_statuses() {
        let phases = vec![
            DrillPhase::passed("a", "kind", Instant::now(), "ok", None),
            DrillPhase::failed("b", "kind", Instant::now(), "bad", None),
            DrillPhase::skipped("c", "kind", "skip"),
        ];
        let summary = summarize_phases(&phases);
        assert_eq!(summary.passed, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 1);
    }

    #[test]
    fn redacts_report_payloads() {
        let options = DrillRunOptions {
            app: "fixture".to_string(),
            require_frontend: false,
            require_drive_bridge: false,
            timeout_seconds: 1,
            interval_ms: 100,
            report: PathBuf::from("report.json"),
            selector: None,
            expect_text: None,
            script: None,
            json: false,
            environment: Vec::new(),
            command: vec!["fixture".to_string()],
        };
        let mut report = DrillReport::new(&options, &[]);
        report.push_phase(DrillPhase::passed(
            "secret-phase",
            "test",
            Instant::now(),
            "ok",
            Some(json!({ "token": "abc", "nested": { "api_key": "def" } })),
        ));
        let value = redacted_report_value(&report).unwrap();
        assert_eq!(
            value["phases"][0]["result"]["token"],
            Value::String("[REDACTED]".to_string())
        );
        assert_eq!(
            value["phases"][0]["result"]["nested"]["api_key"],
            Value::String("[REDACTED]".to_string())
        );
    }

    #[test]
    fn parses_command_hook_script_schema() {
        let script: DrillScript = serde_json::from_value(json!({
            "setup": [{
                "name": "Seed data",
                "run": "npm",
                "args": ["run", "seed"],
                "timeoutMs": 30000,
                "cwd": "."
            }],
            "gates": [{
                "name": "Approve OAuth",
                "instructions": "Approve the GitHub device-code flow in the app or browser.",
                "selector": "#status",
                "expectText": "Signed in",
                "timeoutMs": 120000,
                "clipboard": {
                    "label": "Device code",
                    "value": "ABCD-1234",
                    "sensitive": true,
                    "copy": "attempt"
                },
                "choices": [
                    { "id": "done", "label": "Done", "outcome": "continue" },
                    { "id": "blocked", "label": "Blocked", "outcome": "fail" }
                ],
                "inputs": [
                    {
                        "id": "external-error",
                        "label": "External error message",
                        "kind": "multilineText",
                        "required": false
                    }
                ]
            }],
            "teardown": [{
                "name": "Clean data",
                "run": "npm",
                "args": ["run", "cleanup"],
                "always": true
            }]
        }))
        .unwrap();

        assert_eq!(script.setup.len(), 1);
        assert_eq!(script.gates.len(), 1);
        assert_eq!(script.setup[0].name, "Seed data");
        assert_eq!(script.setup[0].args, vec!["run", "seed"]);
        assert_eq!(script.setup[0].timeout_ms, 30_000);
        assert_eq!(script.setup[0].cwd, PathBuf::from("."));
        assert_eq!(script.gates[0].name, "Approve OAuth");
        assert!(script.gates[0].manual_continue);
        assert_eq!(script.gates[0].timeout_ms, 120_000);
        assert_eq!(script.gates[0].choices.len(), 2);
        assert_eq!(
            script.gates[0].choices[1].outcome,
            DrillGateChoiceOutcome::Fail
        );
        assert_eq!(
            script.gates[0]
                .clipboard
                .as_ref()
                .map(|clipboard| clipboard.copy),
            Some(DrillGateClipboardCopyMode::Attempt)
        );
        assert_eq!(script.gates[0].inputs.len(), 1);
        assert!(script.gates[0].inputs[0].sensitive);
        assert_eq!(script.teardown[0].timeout_ms, DEFAULT_HOOK_TIMEOUT_MS);
        assert!(script.teardown[0].always);
    }

    #[test]
    fn resolves_manual_gate_defaults() {
        let gate = resolve_gate(DrillGate {
            name: "Approve OAuth".to_string(),
            instructions: "Approve the browser handoff.".to_string(),
            selector: None,
            expect_text: None,
            timeout_ms: default_gate_timeout_ms(),
            manual_continue: default_manual_continue(),
            choices: Vec::new(),
            inputs: Vec::new(),
            clipboard: None,
        })
        .unwrap();

        assert_eq!(gate.name, "Approve OAuth");
        assert!(gate.manual_continue);
        assert_eq!(gate.timeout_ms, 300_000);
    }

    #[test]
    fn rejects_gate_expected_text_without_selector() {
        let error = resolve_gate(DrillGate {
            name: "Bad gate".to_string(),
            instructions: "Wait for something.".to_string(),
            selector: None,
            expect_text: Some("Done".to_string()),
            timeout_ms: 1_000,
            manual_continue: true,
            choices: Vec::new(),
            inputs: Vec::new(),
            clipboard: None,
        })
        .unwrap_err();

        assert!(error.to_string().contains("expectText requires selector"));
    }

    #[test]
    fn redacts_sensitive_gate_input_reports() {
        let input = DrillGateInput {
            id: "external-error".to_string(),
            label: "External error message".to_string(),
            kind: DrillGateInputKind::MultilineText,
            required: false,
            sensitive: true,
        };

        let report = gate_input_report(&input, "SSO error details".to_string());

        assert!(report.redacted);
        assert_eq!(report.value, Some("[REDACTED]".to_string()));
    }

    #[test]
    fn prequeued_gate_response_completes_manual_choice_gate() {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender.send("done".to_string()).unwrap();
        sender
            .send("approved in external browser".to_string())
            .unwrap();

        let gate = ResolvedDrillGate {
            name: "Approve OAuth".to_string(),
            instructions: "Approve the browser handoff.".to_string(),
            selector: None,
            expect_text: None,
            timeout_ms: 1_000,
            manual_continue: true,
            choices: vec![DrillGateChoice {
                id: "done".to_string(),
                label: "Done".to_string(),
                outcome: DrillGateChoiceOutcome::Continue,
            }],
            inputs: vec![DrillGateInput {
                id: "external-note".to_string(),
                label: "External note".to_string(),
                kind: DrillGateInputKind::Text,
                required: false,
                sensitive: true,
            }],
            clipboard: None,
        };

        let outcome = execute_gate(
            &gate,
            &discovered_app("2026-06-27T22:00:00Z", "2026-06-27T22:00:01Z"),
            None,
            Some(&receiver),
        )
        .unwrap();

        let GateOutcome::Passed(report) = outcome else {
            panic!("expected manual gate to pass");
        };
        assert_eq!(report.satisfied_by, DrillGateSatisfiedBy::Choice);
        assert_eq!(
            report
                .selected_choice
                .as_ref()
                .map(|choice| choice.id.as_str()),
            Some("done")
        );
        assert_eq!(report.inputs.len(), 1);
        assert_eq!(report.inputs[0].value, Some("[REDACTED]".to_string()));
    }

    #[test]
    fn redacts_sensitive_clipboard_reports() {
        let clipboard = DrillGateClipboard {
            label: "Device code".to_string(),
            value: "ABCD-1234".to_string(),
            sensitive: true,
            copy: DrillGateClipboardCopyMode::ManualOnly,
        };

        let report = DrillGateClipboardReport {
            label: clipboard.label.clone(),
            sensitive: clipboard.sensitive,
            redacted: clipboard.sensitive,
            value: if clipboard.sensitive {
                Some("[REDACTED]".to_string())
            } else {
                Some(clipboard.value.clone())
            },
            copy: clipboard.copy,
            status: DrillGateClipboardStatus::ManualOnly,
            error: None,
        };

        assert!(report.redacted);
        assert_eq!(report.value, Some("[REDACTED]".to_string()));
    }

    #[test]
    fn rejects_unknown_script_fields() {
        let error = serde_json::from_value::<DrillScript>(json!({
            "setup": [],
            "future": []
        }))
        .unwrap_err();

        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn rejects_hook_cwd_outside_workspace() {
        let workspace = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let workspace_root = workspace.path().canonicalize().unwrap();
        let hook = DrillHook {
            name: "Bad cwd".to_string(),
            run: "npm".to_string(),
            args: Vec::new(),
            timeout_ms: 1_000,
            cwd: outside.path().to_path_buf(),
            always: false,
        };

        let error = resolve_hook(&workspace_root, hook, DrillHookLifecycle::Setup).unwrap_err();

        assert!(error.to_string().contains("invalid cwd"));
    }

    #[test]
    fn truncates_hook_output_on_character_boundaries() {
        let workspace = TempDir::new().unwrap();
        let hook = resolved_hook("Verbose", workspace.path());

        let report = hook_report(
            &hook,
            DrillHookLifecycle::Setup,
            Instant::now(),
            Some(0),
            false,
            "a🦀b".repeat(HOOK_OUTPUT_MAX_CHARS),
            "",
        );

        assert_eq!(report.stdout.chars().count(), HOOK_OUTPUT_MAX_CHARS);
        assert!(report.stdout.ends_with('🦀') || report.stdout.ends_with('a'));
    }

    #[test]
    fn hook_failures_map_to_stable_exit_codes() {
        let mut exit_code = EXIT_PASSED;
        apply_hook_exit(
            &mut exit_code,
            DrillHookLifecycle::Setup,
            HookExitKind::Failed,
        );
        assert_eq!(exit_code, EXIT_CHECK_FAILED);

        let mut exit_code = EXIT_PASSED;
        apply_hook_exit(
            &mut exit_code,
            DrillHookLifecycle::Teardown,
            HookExitKind::Failed,
        );
        assert_eq!(exit_code, EXIT_CLEANUP_FAILED);

        let mut exit_code = EXIT_PASSED;
        apply_hook_exit(
            &mut exit_code,
            DrillHookLifecycle::Teardown,
            HookExitKind::TimedOut,
        );
        assert_eq!(exit_code, EXIT_TIMEOUT);
    }

    #[test]
    fn teardown_failure_preserves_prior_app_failure() {
        let mut exit_code = EXIT_APP_EXITED;

        apply_hook_exit(
            &mut exit_code,
            DrillHookLifecycle::Teardown,
            HookExitKind::Failed,
        );

        assert_eq!(exit_code, EXIT_APP_EXITED);
    }

    #[test]
    fn teardown_always_hooks_run_after_prior_teardown_failure() {
        let workspace = TempDir::new().unwrap();
        let marker = workspace.path().join("always-ran.txt");
        let (fail_run, fail_args) = failing_command();
        let (mark_run, mark_args) = write_marker_command(&marker);
        let script = ResolvedDrillScript {
            path: workspace.path().join("drill.json"),
            workspace_root: workspace.path().to_path_buf(),
            setup: Vec::new(),
            gates: Vec::new(),
            teardown: vec![
                ResolvedDrillHook {
                    name: "Fail cleanup".to_string(),
                    run: fail_run,
                    args: fail_args,
                    timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
                    cwd: workspace.path().to_path_buf(),
                    always: false,
                },
                ResolvedDrillHook {
                    name: "Always cleanup".to_string(),
                    run: mark_run,
                    args: mark_args,
                    timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
                    cwd: workspace.path().to_path_buf(),
                    always: true,
                },
            ],
        };
        let options = test_options(Some(script.path.clone()));
        let mut report = DrillReport::new(&options, &[]);
        report.set_script(&script);
        let mut exit_code = EXIT_PASSED;

        let error = run_hooks(
            &script,
            DrillHookLifecycle::Teardown,
            &mut report,
            &mut exit_code,
        )
        .unwrap_err();

        assert!(error.to_string().contains("Fail cleanup"));
        assert_eq!(exit_code, EXIT_CLEANUP_FAILED);
        assert!(marker.exists());
        assert_eq!(report.script.as_ref().unwrap().teardown.len(), 2);
    }

    #[test]
    fn aggregates_hook_results_into_report() {
        let workspace = TempDir::new().unwrap();
        let script = ResolvedDrillScript {
            path: workspace.path().join("drill.json"),
            workspace_root: workspace.path().to_path_buf(),
            setup: Vec::new(),
            gates: Vec::new(),
            teardown: Vec::new(),
        };
        let options = test_options(Some(script.path.clone()));
        let mut report = DrillReport::new(&options, &[]);
        report.set_script(&script);
        let setup_hook = hook_report(
            &resolved_hook("Seed", workspace.path()),
            DrillHookLifecycle::Setup,
            Instant::now(),
            Some(0),
            false,
            "seeded",
            "",
        );
        let teardown_hook = hook_report(
            &resolved_hook("Clean", workspace.path()),
            DrillHookLifecycle::Teardown,
            Instant::now(),
            Some(1),
            false,
            "",
            "failed",
        );

        report.push_hook(&script, setup_hook);
        report.push_hook(&script, teardown_hook);

        let value = redacted_report_value(&report).unwrap();
        assert_eq!(value["script"]["setup"][0]["name"], "Seed");
        assert_eq!(value["script"]["setup"][0]["stdout"], "seeded");
        assert_eq!(value["script"]["teardown"][0]["name"], "Clean");
        assert_eq!(value["script"]["teardown"][0]["exitCode"], 1);
    }

    #[test]
    fn new_run_selector_accepts_started_or_heartbeat_after_spawn() {
        let started_at = OffsetDateTime::parse("2026-06-27T22:00:00Z", &Rfc3339).unwrap();
        let app = discovered_app("2026-06-27T21:59:00Z", "2026-06-27T22:00:01Z");

        assert!(app_started_or_heartbeat_after(&app, started_at));
    }

    #[test]
    fn new_run_selector_rejects_pre_spawn_discovery_times() {
        let started_at = OffsetDateTime::parse("2026-06-27T22:00:00Z", &Rfc3339).unwrap();
        let app = discovered_app("2026-06-27T21:59:00Z", "2026-06-27T21:59:30Z");

        assert!(!app_started_or_heartbeat_after(&app, started_at));
    }

    fn test_options(script: Option<PathBuf>) -> DrillRunOptions {
        DrillRunOptions {
            app: "fixture".to_string(),
            require_frontend: false,
            require_drive_bridge: false,
            timeout_seconds: 1,
            interval_ms: 100,
            report: PathBuf::from("report.json"),
            selector: None,
            expect_text: None,
            script,
            json: false,
            environment: Vec::new(),
            command: vec!["fixture".to_string()],
        }
    }

    fn resolved_hook(name: &str, cwd: &Path) -> ResolvedDrillHook {
        ResolvedDrillHook {
            name: name.to_string(),
            run: "fixture".to_string(),
            args: Vec::new(),
            timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
            cwd: cwd.to_path_buf(),
            always: false,
        }
    }

    fn discovered_app(started_at: &str, last_heartbeat_at: &str) -> DiscoveredApp {
        DiscoveredApp {
            instance_id: "instance".to_string(),
            session_id: "session".to_string(),
            service_name: "fixture".to_string(),
            service_version: None,
            app_identifier: None,
            pid: 42,
            started_at: started_at.to_string(),
            last_heartbeat_at: last_heartbeat_at.to_string(),
            heartbeat_age_seconds: Some(0),
            status: DiscoveryStatus::Active,
            capabilities: Vec::new(),
            database_path: "telemetry.sqlite".to_string(),
            database_readable: true,
            schema_valid: true,
            discovery_path: "discovery.json".to_string(),
            stale_reason: None,
            superseded_by_session_id: None,
            seconds_until_next_start: None,
            churn_session_count: None,
            churn_window_seconds: None,
            churn_hint: None,
        }
    }

    #[cfg(windows)]
    fn failing_command() -> (String, Vec<String>) {
        (
            "powershell".to_string(),
            vec![
                "-NoProfile".to_string(),
                "-Command".to_string(),
                "exit 1".to_string(),
            ],
        )
    }

    #[cfg(not(windows))]
    fn failing_command() -> (String, Vec<String>) {
        (
            "sh".to_string(),
            vec!["-c".to_string(), "exit 1".to_string()],
        )
    }

    #[cfg(windows)]
    fn write_marker_command(marker: &Path) -> (String, Vec<String>) {
        (
            "powershell".to_string(),
            vec![
                "-NoProfile".to_string(),
                "-Command".to_string(),
                format!(
                    "Set-Content -LiteralPath '{}' -Value ran",
                    marker.display().to_string().replace('\'', "''")
                ),
            ],
        )
    }

    #[cfg(not(windows))]
    fn write_marker_command(marker: &Path) -> (String, Vec<String>) {
        (
            "sh".to_string(),
            vec![
                "-c".to_string(),
                format!(
                    "printf ran > '{}'",
                    marker.display().to_string().replace('\'', "'\\''")
                ),
            ],
        )
    }
}
