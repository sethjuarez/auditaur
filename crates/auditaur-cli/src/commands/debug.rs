use std::{
    collections::HashSet,
    env, fs,
    io::{Read, Seek, SeekFrom, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[cfg(not(windows))]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

use anyhow::{anyhow, Context, Result};
use auditaur_core::{
    drive_bridge::{
        DriveBridgeStatus, DRIVE_BRIDGE_DIR, DRIVE_BRIDGE_STALE_FILE_NANOS,
        DRIVE_BRIDGE_STATUS_FILE,
    },
    model::TelemetrySource,
    storage::{
        FrontendErrorQuery, LogQuery, SpanEventQuery, SpanQuery, TauriEventQuery, TauriIpcQuery,
        TauriWindowQuery,
    },
};
use serde::Serialize;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::{
    commands::read,
    discovery::{self, DiscoveredApp, DiscoveryStatus},
    output::table_cell,
};

#[derive(Debug, Clone)]
pub struct DebugSelector {
    pub db: Option<PathBuf>,
    pub app: Option<String>,
    pub session_id: Option<String>,
    pub instance_id: Option<String>,
    pub pid: Option<u32>,
    pub latest: bool,
    pub active: bool,
    pub cdp_port: Option<u16>,
    pub require_frontend: bool,
    pub require_drive_bridge: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugStatus {
    pub schema_version: u8,
    pub generated_at_unix_nanos: i64,
    pub ready: bool,
    pub app: Option<DiscoveredApp>,
    pub database_path: Option<String>,
    pub session_id: Option<String>,
    pub stages: Vec<DebugStage>,
    pub telemetry: DebugTelemetryCounts,
    pub cdp: Option<DebugCdpStatus>,
    pub hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebugStage {
    pub name: String,
    pub status: DebugStageStatus,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DebugStageStatus {
    Ok,
    Waiting,
    Error,
    Skipped,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugTelemetryCounts {
    pub sessions: usize,
    pub logs: usize,
    pub spans: usize,
    pub span_events: usize,
    pub frontend_errors: usize,
    pub ipc: usize,
    pub events: usize,
    pub windows: usize,
    pub frontend_records: usize,
    pub backend_records: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugCdpStatus {
    pub port: u16,
    pub ok: bool,
    pub target_count: Option<usize>,
    pub browser: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DebugRunResult {
    schema_version: u8,
    generated_at_unix_nanos: i64,
    app: Option<DebugRunApp>,
    status: DebugStatus,
    process: DebugProcessStatus,
    selectors: DebugRunSelectors,
    session_file: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DebugRunApp {
    service_name: String,
    session_id: String,
    instance_id: String,
    pid: u32,
    database_path: String,
    capabilities: Vec<String>,
    pinned_by: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DebugProcessStatus {
    command: Vec<String>,
    pid: Option<u32>,
    exit_code: Option<i32>,
    running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stdout_tail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stderr_tail: Option<String>,
}

impl DebugProcessStatus {
    fn with_output(mut self, output: &ChildOutputTail) -> Self {
        self.stdout_tail = output.stdout.clone();
        self.stderr_tail = output.stderr.clone();
        self
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct DebugRunSelectors {
    debug: Vec<String>,
    drive: Vec<String>,
    read: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DebugRunOutput {
    Human,
    JsonLines,
    Quiet,
}

#[derive(Debug, Clone, Default)]
struct ChildOutputTail {
    stdout: Option<String>,
    stderr: Option<String>,
}

struct CapturedChildOutput {
    stdout: Option<fs::File>,
    stderr: Option<fs::File>,
}

impl CapturedChildOutput {
    fn prepare(enabled: bool) -> Result<Self> {
        if !enabled {
            return Ok(Self {
                stdout: None,
                stderr: None,
            });
        }
        Ok(Self {
            stdout: Some(tempfile::tempfile().context("failed to create child stdout capture")?),
            stderr: Some(tempfile::tempfile().context("failed to create child stderr capture")?),
        })
    }

    fn stdout_stdio(&self) -> Result<Stdio> {
        output_file_stdio(&self.stdout)
    }

    fn stderr_stdio(&self) -> Result<Stdio> {
        output_file_stdio(&self.stderr)
    }

    fn collect(&mut self) -> ChildOutputTail {
        ChildOutputTail {
            stdout: self
                .stdout
                .take()
                .and_then(read_file_tail)
                .filter(|output| !output.is_empty()),
            stderr: self
                .stderr
                .take()
                .and_then(read_file_tail)
                .filter(|output| !output.is_empty()),
        }
    }
}

fn output_file_stdio(file: &Option<fs::File>) -> Result<Stdio> {
    let file = file
        .as_ref()
        .ok_or_else(|| anyhow!("child output capture was not initialized"))?;
    Ok(Stdio::from(
        file.try_clone()
            .context("failed to clone child output capture")?,
    ))
}

fn read_file_tail(mut file: fs::File) -> Option<String> {
    const LIMIT: u64 = 32 * 1024;
    let len = file.metadata().ok()?.len();
    if len > LIMIT {
        file.seek(SeekFrom::Start(len - LIMIT)).ok()?;
    } else {
        file.seek(SeekFrom::Start(0)).ok()?;
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).trim().to_string())
}

fn format_child_output_for_error(output: &ChildOutputTail) -> String {
    let mut details = Vec::new();
    if let Some(stdout) = &output.stdout {
        details.push(format!("stdout tail:\n{stdout}"));
    }
    if let Some(stderr) = &output.stderr {
        details.push(format!("stderr tail:\n{stderr}"));
    }
    if details.is_empty() {
        String::new()
    } else {
        format!("\n\n{}", details.join("\n\n"))
    }
}

pub fn status(selector: DebugSelector, json: bool) -> Result<()> {
    let status = snapshot(&selector)?;
    read::print_json_or_table(json, &status, || print_status(&status))
}

pub fn watch(
    selector: DebugSelector,
    interval_ms: u64,
    timeout_seconds: Option<u64>,
    until_ready: bool,
    json: bool,
) -> Result<()> {
    let started = Instant::now();
    let interval = Duration::from_millis(interval_ms).max(Duration::from_millis(100));
    loop {
        let status = snapshot(&selector)?;
        if json {
            println!("{}", serde_json::to_string(&status)?);
        } else {
            print_status(&status)?;
            println!();
        }
        if until_ready && status.ready {
            return Ok(());
        }
        if !until_ready {
            return Ok(());
        }
        if timeout_seconds.is_some_and(|seconds| started.elapsed() >= Duration::from_secs(seconds))
        {
            return Err(anyhow!(
                "Timed out after {}s waiting for Auditaur debug readiness.",
                timeout_seconds.unwrap()
            ));
        }
        thread::sleep(interval);
    }
}

pub fn run(
    selector: DebugSelector,
    interval_ms: u64,
    timeout_seconds: Option<u64>,
    json: bool,
    write_session: Option<PathBuf>,
    environment: Vec<(String, String)>,
    command: Vec<String>,
) -> Result<DebugRunResult> {
    let output = if json {
        DebugRunOutput::JsonLines
    } else {
        DebugRunOutput::Human
    };
    run_with_output(
        selector,
        interval_ms,
        timeout_seconds,
        output,
        write_session,
        environment,
        command,
        "Auditaur debug session ready.",
    )
}

pub(crate) fn run_with_output(
    selector: DebugSelector,
    interval_ms: u64,
    timeout_seconds: Option<u64>,
    output: DebugRunOutput,
    write_session: Option<PathBuf>,
    environment: Vec<(String, String)>,
    command: Vec<String>,
    handoff_title: &str,
) -> Result<DebugRunResult> {
    if command.is_empty() {
        return Err(anyhow!(
            "`auditaur debug run` requires a command after `--`."
        ));
    }
    let mut child_command = command_from_argv(&command);
    child_command.envs(environment);
    let mut captured_output = CapturedChildOutput::prepare(output != DebugRunOutput::Human)?;
    if output != DebugRunOutput::Human {
        child_command
            .stdout(captured_output.stdout_stdio()?)
            .stderr(captured_output.stderr_stdio()?);
        #[cfg(windows)]
        child_command.creation_flags(0x0800_0000);
    } else {
        child_command
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
    }
    #[cfg(not(windows))]
    child_command.process_group(0);
    let preexisting = if selector.db.is_none() {
        selector
            .app
            .as_deref()
            .map(matching_apps)
            .transpose()?
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let started_at = OffsetDateTime::now_utc();
    let mut child = child_command
        .spawn()
        .with_context(|| format!("failed to start `{}`", command.join(" ")))?;
    let pid = child.id();
    let started = Instant::now();
    let interval = Duration::from_millis(interval_ms).max(Duration::from_millis(100));
    let mut active_selector = selector.clone();
    let mut pinned_app = None;
    if selector.db.is_none() {
        if let Some(app_name) = selector.app.as_deref() {
            let preexisting_ids = preexisting
                .iter()
                .map(|app| app.instance_id.as_str())
                .collect::<HashSet<_>>();
            match wait_for_spawned_app(
                app_name,
                started_at,
                &preexisting_ids,
                started,
                timeout_seconds,
                interval,
                &mut child,
            )? {
                SpawnedAppOutcome::Pinned(app) => {
                    active_selector = pinned_selector(&selector, &app);
                    pinned_app = Some(app);
                }
                SpawnedAppOutcome::Exited(status) => {
                    let child_output = captured_output.collect();
                    return Err(anyhow!(
                        "debug command exited before a new Auditaur session was discovered with status {status}{}",
                        format_child_output_for_error(&child_output)
                    ));
                }
                SpawnedAppOutcome::TimedOut => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let child_output = captured_output.collect();
                    return Err(anyhow!(
                        "Timed out after {}s waiting for a new Auditaur session after startup.{}",
                        timeout_seconds.unwrap(),
                        format_child_output_for_error(&child_output)
                    ));
                }
            }
        }
    }
    let mut final_status;
    loop {
        final_status = match snapshot(&active_selector) {
            Ok(status) => status,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let child_output = captured_output.collect();
                return Err(error.context(format!(
                    "debug readiness snapshot failed after starting command{}",
                    format_child_output_for_error(&child_output)
                )));
            }
        };
        if output == DebugRunOutput::JsonLines {
            println!("{}", serde_json::to_string(&final_status)?);
        } else if output == DebugRunOutput::Human {
            print_status(&final_status)?;
            println!();
        }
        if final_status.ready {
            break;
        }
        if let Some(status) = child.try_wait()? {
            let process = DebugProcessStatus {
                command,
                pid: Some(pid),
                exit_code: status.code(),
                running: false,
                stdout_tail: None,
                stderr_tail: None,
            };
            let child_output = captured_output.collect();
            let result = debug_run_result(
                final_status,
                process.with_output(&child_output),
                pinned_app.as_ref(),
                &write_session,
            );
            if output == DebugRunOutput::JsonLines {
                println!("{}", serde_json::to_string(&result)?);
            }
            return Err(anyhow!(
                "debug command exited before Auditaur became ready with status {status}{}",
                format_child_output_for_error(&child_output)
            ));
        }
        if timeout_seconds.is_some_and(|seconds| started.elapsed() >= Duration::from_secs(seconds))
        {
            let _ = child.kill();
            let _ = child.wait();
            let child_output = captured_output.collect();
            return Err(anyhow!(
                "Timed out after {}s waiting for Auditaur debug readiness.{}",
                timeout_seconds.unwrap(),
                format_child_output_for_error(&child_output)
            ));
        }
        thread::sleep(interval);
    }

    let child_status = child.try_wait()?;
    if child_status.is_some() {
        let _ = captured_output.collect();
    }
    let process = DebugProcessStatus {
        command,
        pid: Some(pid),
        exit_code: child_status.and_then(|status| status.code()),
        running: child_status.is_none(),
        stdout_tail: None,
        stderr_tail: None,
    };
    let result = debug_run_result(final_status, process, pinned_app.as_ref(), &write_session);
    write_session_file(write_session.as_deref(), &result)?;
    if output == DebugRunOutput::JsonLines {
        println!("{}", serde_json::to_string(&result)?);
    } else if output == DebugRunOutput::Human {
        print_run_handoff(handoff_title, &result);
    }
    Ok(result)
}

fn command_from_argv(command: &[String]) -> Command {
    let program = resolve_program(&command[0]).unwrap_or_else(|| PathBuf::from(&command[0]));
    let mut child_command = Command::new(program);
    child_command.args(&command[1..]);
    child_command
}

fn resolve_program(program: &str) -> Option<PathBuf> {
    #[cfg(not(windows))]
    {
        let _ = program;
        return None;
    }

    #[cfg(windows)]
    {
        let path = Path::new(program);
        if has_path_separator(program) || path.is_absolute() {
            return executable_candidates(path)
                .into_iter()
                .find(|candidate| candidate.is_file());
        }

        let path_var = env::var_os("PATH")?;
        env::split_paths(&path_var)
            .flat_map(|dir| executable_candidates(&dir.join(program)))
            .find(|candidate| candidate.is_file())
    }
}

fn has_path_separator(program: &str) -> bool {
    program.contains('/') || program.contains('\\')
}

fn executable_candidates(path: &Path) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        if path.extension().is_none() {
            let pathext = env::var_os("PATHEXT")
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string());
            let mut candidates = pathext
                .split(';')
                .filter(|extension| !extension.trim().is_empty())
                .map(|extension| {
                    let extension = extension.trim().trim_start_matches('.');
                    path.with_extension(extension)
                })
                .collect::<Vec<_>>();
            candidates.push(path.to_path_buf());
            return candidates;
        }
    }
    vec![path.to_path_buf()]
}

pub(crate) fn print_run_handoff(title: &str, result: &DebugRunResult) {
    println!("{title}");
    if let Some(path) = &result.session_file {
        println!("Session file: {path}");
    }
    if result.process.running {
        if let Some(pid) = result.process.pid {
            println!("Observed app is still running (pid {pid}).");
        } else {
            println!("Observed app is still running.");
        }
    } else {
        println!("Observed app process exited after readiness.");
    }

    if result.app.is_some() {
        println!("Follow-up commands:");
        println!(
            "  {}",
            command_line("auditaur", ["debug"], &result.selectors.debug, ["status"])
        );
        println!(
            "  {}",
            command_line("auditaur", ["tail"], &result.selectors.read, ["--replay"])
        );
        println!(
            "  {}",
            command_line(
                "auditaur",
                ["logs"],
                &result.selectors.read,
                std::iter::empty::<&str>(),
            )
        );
        if result.app.as_ref().is_some_and(|app| {
            app.capabilities
                .iter()
                .any(|capability| capability == "drive_bridge")
        }) {
            println!(
                "  {}",
                command_line("auditaur", ["drive"], &result.selectors.drive, ["inspect"])
            );
        } else {
            println!(
                "  auditaur drive ... inspect   # after enabling the Tauri-native drive bridge"
            );
        }
    }
    if let Some(path) = &result.session_file {
        println!(
            "  {}",
            command_line("auditaur", ["stop"], &[], ["--session-file", path])
        );
    }
}

fn command_line<'a>(
    program: &str,
    prefix: impl IntoIterator<Item = &'a str>,
    selector: &[String],
    suffix: impl IntoIterator<Item = &'a str>,
) -> String {
    std::iter::once(program.to_string())
        .chain(prefix.into_iter().map(str::to_string))
        .chain(selector.iter().cloned())
        .chain(suffix.into_iter().map(str::to_string))
        .map(|arg| quote_arg(&arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_arg(arg: &str) -> String {
    if arg.chars().all(|character| {
        character.is_ascii_alphanumeric()
            || matches!(character, '-' | '_' | '.' | '/' | '\\' | ':' | '=')
    }) {
        arg.to_string()
    } else {
        format!("\"{}\"", arg.replace('"', "\\\""))
    }
}

enum SpawnedAppOutcome {
    Pinned(DiscoveredApp),
    Exited(std::process::ExitStatus),
    TimedOut,
}

fn wait_for_spawned_app(
    app_name: &str,
    started_at: OffsetDateTime,
    preexisting_ids: &HashSet<&str>,
    started: Instant,
    timeout_seconds: Option<u64>,
    interval: Duration,
    child: &mut Child,
) -> Result<SpawnedAppOutcome> {
    loop {
        if let Some(app) = find_spawned_app(app_name, started_at, preexisting_ids)? {
            return Ok(SpawnedAppOutcome::Pinned(app));
        }
        if let Some(status) = child.try_wait()? {
            return Ok(SpawnedAppOutcome::Exited(status));
        }
        if timeout_seconds.is_some_and(|seconds| started.elapsed() >= Duration::from_secs(seconds))
        {
            return Ok(SpawnedAppOutcome::TimedOut);
        }
        thread::sleep(interval);
    }
}

fn matching_apps(app_name: &str) -> Result<Vec<DiscoveredApp>> {
    discovery::list_apps().map(|apps| {
        apps.into_iter()
            .filter(|app| app_matches(app, app_name))
            .collect()
    })
}

fn find_spawned_app(
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

fn parse_rfc3339(value: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).ok()
}

fn pinned_selector(base: &DebugSelector, app: &DiscoveredApp) -> DebugSelector {
    DebugSelector {
        db: Some(PathBuf::from(&app.database_path)),
        app: None,
        session_id: Some(app.session_id.clone()),
        instance_id: Some(app.instance_id.clone()),
        pid: Some(app.pid),
        latest: false,
        active: false,
        cdp_port: base.cdp_port,
        require_frontend: base.require_frontend,
        require_drive_bridge: base.require_drive_bridge,
    }
}

fn debug_run_result(
    status: DebugStatus,
    process: DebugProcessStatus,
    pinned_app: Option<&DiscoveredApp>,
    session_file: &Option<PathBuf>,
) -> DebugRunResult {
    let app = pinned_app
        .map(|app| DebugRunApp::from_discovered(app, "newDiscoveryAfterSpawn"))
        .or_else(|| {
            status
                .app
                .as_ref()
                .map(|app| DebugRunApp::from_discovered(app, "selector"))
        });
    let selectors = app
        .as_ref()
        .map(DebugRunSelectors::from_app)
        .unwrap_or_default();
    DebugRunResult {
        schema_version: 1,
        generated_at_unix_nanos: read::current_time_unix_nanos(),
        app,
        status,
        process,
        selectors,
        session_file: session_file
            .as_ref()
            .map(|path| path.to_string_lossy().to_string()),
    }
}

fn write_session_file(path: Option<&Path>, result: &DebugRunResult) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create session file directory `{}`",
                parent.display()
            )
        })?;
    }
    fs::write(path, serde_json::to_vec_pretty(result)?)
        .with_context(|| format!("failed to write session file `{}`", path.display()))
}

impl DebugRunApp {
    fn from_discovered(app: &DiscoveredApp, pinned_by: impl Into<String>) -> Self {
        Self {
            service_name: app.service_name.clone(),
            session_id: app.session_id.clone(),
            instance_id: app.instance_id.clone(),
            pid: app.pid,
            database_path: app.database_path.clone(),
            capabilities: app.capabilities.clone(),
            pinned_by: pinned_by.into(),
        }
    }
}

impl DebugRunSelectors {
    fn from_app(app: &DebugRunApp) -> Self {
        Self {
            debug: vec![
                "--db".to_string(),
                app.database_path.clone(),
                "--session-id".to_string(),
                app.session_id.clone(),
                "--instance-id".to_string(),
                app.instance_id.clone(),
                "--pid".to_string(),
                app.pid.to_string(),
            ],
            drive: vec![
                "--session-id".to_string(),
                app.session_id.clone(),
                "--instance-id".to_string(),
                app.instance_id.clone(),
                "--pid".to_string(),
                app.pid.to_string(),
            ],
            read: vec![
                "--db".to_string(),
                app.database_path.clone(),
                "--session".to_string(),
                app.session_id.clone(),
            ],
        }
    }
}

pub(crate) fn snapshot(selector: &DebugSelector) -> Result<DebugStatus> {
    let mut stages = Vec::new();
    let mut hints = Vec::new();
    let app = if selector.db.is_some() {
        stages.push(stage(
            "app_discovery",
            DebugStageStatus::Skipped,
            "using explicit --db; app discovery is not required",
        ));
        stages.push(stage(
            "heartbeat",
            DebugStageStatus::Skipped,
            "using explicit --db; app heartbeat is not required",
        ));
        None
    } else {
        match select_app(selector)? {
            Some(app) => {
                stages.push(stage(
                    "app_discovery",
                    DebugStageStatus::Ok,
                    format!(
                        "discovered {} pid={} session={}",
                        app.service_name, app.pid, app.session_id
                    ),
                ));
                Some(app)
            }
            None => {
                stages.push(stage(
                    "app_discovery",
                    DebugStageStatus::Waiting,
                    "waiting for an Auditaur discovery file",
                ));
                hints.push(
                    "Start the app or use `auditaur debug run -- <command>` to wrap startup."
                        .to_string(),
                );
                None
            }
        }
    };

    let db = selector
        .db
        .clone()
        .or_else(|| app.as_ref().map(|app| PathBuf::from(&app.database_path)));
    let db_for_drive_bridge = db.clone();
    if let Some(app) = &app {
        if app.status == DiscoveryStatus::Active {
            stages.push(stage(
                "heartbeat",
                DebugStageStatus::Ok,
                format!(
                    "heartbeat is fresh{}",
                    app.heartbeat_age_seconds
                        .map(|age| format!(" ({age}s old)"))
                        .unwrap_or_default()
                ),
            ));
        } else {
            stages.push(stage(
                "heartbeat",
                DebugStageStatus::Waiting,
                app.stale_reason
                    .clone()
                    .unwrap_or_else(|| "heartbeat is stale".to_string()),
            ));
        }
    }

    let (database_path, telemetry, session_id) = match db {
        Some(db) => load_database_status(&db, selector, &mut stages, &mut hints)?,
        None => {
            stages.push(stage(
                "telemetry_database",
                DebugStageStatus::Waiting,
                "waiting for a telemetry database path",
            ));
            (None, DebugTelemetryCounts::default(), None)
        }
    };

    push_drive_bridge_stage(
        db_for_drive_bridge.as_deref(),
        selector.require_drive_bridge,
        &mut stages,
        &mut hints,
    );

    if let Some(cdp_port) = selector.cdp_port {
        let cdp = cdp_status(cdp_port);
        stages.push(stage(
            "cdp_endpoint",
            if cdp.ok {
                DebugStageStatus::Ok
            } else {
                DebugStageStatus::Waiting
            },
            cdp.message.clone(),
        ));
        let ready = readiness(
            &stages,
            selector.require_frontend,
            selector.require_drive_bridge,
        );
        Ok(DebugStatus {
            schema_version: 1,
            generated_at_unix_nanos: read::current_time_unix_nanos(),
            ready,
            app,
            database_path,
            session_id,
            stages,
            telemetry,
            cdp: Some(cdp),
            hints,
        })
    } else {
        stages.push(stage(
            "cdp_endpoint",
            DebugStageStatus::Skipped,
            "pass --cdp-port to include WebView/CDP readiness",
        ));
        let ready = readiness(
            &stages,
            selector.require_frontend,
            selector.require_drive_bridge,
        );
        Ok(DebugStatus {
            schema_version: 1,
            generated_at_unix_nanos: read::current_time_unix_nanos(),
            ready,
            app,
            database_path,
            session_id,
            stages,
            telemetry,
            cdp: None,
            hints,
        })
    }
}

fn push_drive_bridge_stage(
    db: Option<&Path>,
    required: bool,
    stages: &mut Vec<DebugStage>,
    hints: &mut Vec<String>,
) {
    let Some(db) = db else {
        stages.push(stage(
            "drive_bridge",
            if required {
                DebugStageStatus::Waiting
            } else {
                DebugStageStatus::Skipped
            },
            "waiting for a telemetry database path before checking drive bridge readiness",
        ));
        return;
    };
    let Some(data_dir) = db.parent() else {
        stages.push(stage(
            "drive_bridge",
            DebugStageStatus::Error,
            format!(
                "telemetry database path has no parent directory: {}",
                db.display()
            ),
        ));
        return;
    };

    let status_path = data_dir
        .join(DRIVE_BRIDGE_DIR)
        .join(DRIVE_BRIDGE_STATUS_FILE);
    if !status_path.is_file() {
        stages.push(stage(
            "drive_bridge",
            if required {
                DebugStageStatus::Waiting
            } else {
                DebugStageStatus::Skipped
            },
            "drive bridge status file has not been written; enable initAuditaur({ driveBridge: true }) to drive the WebView",
        ));
        if required {
            hints.push("Enable `initAuditaur({ driveBridge: true })` in exactly one debug/test WebView for this Auditaur session.".to_string());
        }
        return;
    }

    let status = match std::fs::read_to_string(&status_path)
        .with_context(|| {
            format!(
                "failed to read drive bridge status {}",
                status_path.display()
            )
        })
        .and_then(|contents| {
            serde_json::from_str::<DriveBridgeStatus>(&contents).with_context(|| {
                format!(
                    "failed to parse drive bridge status {}",
                    status_path.display()
                )
            })
        }) {
        Ok(status) => status,
        Err(error) => {
            stages.push(stage(
                "drive_bridge",
                DebugStageStatus::Error,
                format!("drive bridge status is unreadable: {error}"),
            ));
            return;
        }
    };

    let heartbeat_age_nanos = read::current_time_unix_nanos() - status.last_heartbeat_unix_nanos;
    let heartbeat_fresh = heartbeat_age_nanos <= DRIVE_BRIDGE_STALE_FILE_NANOS;
    let actionable = status.active && heartbeat_fresh;
    let stage_status = if actionable {
        DebugStageStatus::Ok
    } else if required {
        DebugStageStatus::Waiting
    } else {
        DebugStageStatus::Skipped
    };
    let message = if !status.active {
        "drive bridge is registered but inactive".to_string()
    } else if !heartbeat_fresh {
        "drive bridge heartbeat is stale".to_string()
    } else {
        format!(
            "drive bridge is active{}",
            status
                .window_label
                .as_ref()
                .map(|label| format!(" for window `{label}`"))
                .unwrap_or_default()
        )
    };
    stages.push(stage("drive_bridge", stage_status, message));
}

fn load_database_status(
    db: &Path,
    selector: &DebugSelector,
    stages: &mut Vec<DebugStage>,
    hints: &mut Vec<String>,
) -> Result<(Option<String>, DebugTelemetryCounts, Option<String>)> {
    if !db.is_file() {
        stages.push(stage(
            "telemetry_database",
            DebugStageStatus::Waiting,
            format!("waiting for telemetry database: {}", db.display()),
        ));
        return Ok((
            Some(db.to_string_lossy().to_string()),
            DebugTelemetryCounts::default(),
            None,
        ));
    }

    let store = match read::open_validated_store(db) {
        Ok(store) => {
            stages.push(stage(
                "telemetry_database",
                DebugStageStatus::Ok,
                format!("database is readable and schema-valid: {}", db.display()),
            ));
            store
        }
        Err(error) => {
            stages.push(stage(
                "telemetry_database",
                DebugStageStatus::Error,
                format!("database could not be opened or validated: {error}"),
            ));
            return Ok((
                Some(db.to_string_lossy().to_string()),
                DebugTelemetryCounts::default(),
                None,
            ));
        }
    };

    let sessions = store.list_sessions(Some(50))?;
    let session_id = selector
        .session_id
        .clone()
        .or_else(|| sessions.first().map(|session| session.id.clone()));
    if let Some(session_id) = &session_id {
        stages.push(stage(
            "session",
            DebugStageStatus::Ok,
            format!("session is queryable: {session_id}"),
        ));
    } else {
        stages.push(stage(
            "session",
            DebugStageStatus::Waiting,
            "waiting for the first session row",
        ));
    }

    let telemetry = telemetry_counts(&store, session_id.clone())?;
    if telemetry.windows > 0 {
        stages.push(stage(
            "window",
            DebugStageStatus::Ok,
            format!("{} window record(s) captured", telemetry.windows),
        ));
    } else {
        stages.push(stage(
            "window",
            DebugStageStatus::Waiting,
            "waiting for Tauri window telemetry",
        ));
    }

    if telemetry.backend_records > 0 {
        stages.push(stage(
            "backend_telemetry",
            DebugStageStatus::Ok,
            format!(
                "{} backend/plugin record(s) captured",
                telemetry.backend_records
            ),
        ));
    } else {
        stages.push(stage(
            "backend_telemetry",
            DebugStageStatus::Waiting,
            "waiting for backend/plugin telemetry rows",
        ));
    }

    if telemetry.frontend_records > 0 {
        stages.push(stage(
            "frontend_telemetry",
            DebugStageStatus::Ok,
            format!("{} frontend record(s) captured", telemetry.frontend_records),
        ));
    } else {
        stages.push(stage(
            "frontend_telemetry",
            if selector.require_frontend {
                DebugStageStatus::Waiting
            } else {
                DebugStageStatus::Skipped
            },
            "no frontend telemetry rows observed yet",
        ));
        hints.push(
            "If frontend telemetry is expected, click a webview action and inspect app UI/export errors."
                .to_string(),
        );
    }

    Ok((
        Some(db.to_string_lossy().to_string()),
        DebugTelemetryCounts {
            sessions: sessions.len(),
            ..telemetry
        },
        session_id,
    ))
}

fn telemetry_counts(
    store: &auditaur_collector::exporter_sqlite::SqliteStore,
    session_id: Option<String>,
) -> Result<DebugTelemetryCounts> {
    let logs = store.list_logs(&LogQuery {
        session_id: session_id.clone(),
        trace_id: None,
        limit: Some(usize::MAX),
    })?;
    let spans = store.list_spans(&SpanQuery {
        session_id: session_id.clone(),
        trace_id: None,
        limit: Some(usize::MAX),
    })?;
    let span_events = store.list_span_events(&SpanEventQuery {
        session_id: session_id.clone(),
        trace_id: None,
        limit: Some(usize::MAX),
    })?;
    let frontend_errors = store.list_frontend_errors(&FrontendErrorQuery {
        session_id: session_id.clone(),
        trace_id: None,
        limit: Some(usize::MAX),
    })?;
    let ipc = store.list_tauri_ipc_calls(&TauriIpcQuery {
        session_id: session_id.clone(),
        trace_id: None,
        limit: Some(usize::MAX),
    })?;
    let events = store.list_tauri_events(&TauriEventQuery {
        session_id: session_id.clone(),
        trace_id: None,
        limit: Some(usize::MAX),
    })?;
    let windows = store.list_tauri_windows(&TauriWindowQuery {
        session_id,
        latest_only: false,
        limit: Some(usize::MAX),
    })?;
    let frontend_logs = logs
        .iter()
        .filter(|log| log.source == TelemetrySource::Frontend)
        .count();
    let backend_logs = logs.len().saturating_sub(frontend_logs);
    let frontend_spans = spans
        .iter()
        .filter(|span| span.source == TelemetrySource::Frontend)
        .count();
    let backend_spans = spans.len().saturating_sub(frontend_spans);
    Ok(DebugTelemetryCounts {
        sessions: 0,
        logs: logs.len(),
        spans: spans.len(),
        span_events: span_events.len(),
        frontend_errors: frontend_errors.len(),
        ipc: ipc.len(),
        events: events.len(),
        windows: windows.len(),
        frontend_records: frontend_logs
            + frontend_spans
            + frontend_errors.len()
            + ipc.len()
            + events.len(),
        backend_records: backend_logs + backend_spans + span_events.len() + windows.len(),
    })
}

fn select_app(selector: &DebugSelector) -> Result<Option<DiscoveredApp>> {
    let mut candidates: Vec<_> = discovery::list_apps()?
        .into_iter()
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
        return Ok(candidates.into_iter().next());
    }
    let active_count = candidates
        .iter()
        .filter(|candidate| candidate.status == DiscoveryStatus::Active)
        .count();
    match candidates.as_slice() {
        [] => Ok(None),
        [candidate] => Ok(Some(candidate.clone())),
        _ if active_count == 1 => Ok(candidates
            .into_iter()
            .find(|candidate| candidate.status == DiscoveryStatus::Active)),
        _ => Err(anyhow!(
            "Multiple Auditaur apps matched debug selector. Pass --session-id, --instance-id, --pid, --latest, or --active."
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

fn cdp_status(port: u16) -> DebugCdpStatus {
    match get_cdp_json(port, "/json/version")
        .and_then(|version| get_cdp_json(port, "/json/list").map(|targets| (version, targets)))
    {
        Ok((version, targets)) => {
            let target_count = targets.as_array().map(Vec::len);
            let browser = version
                .get("Browser")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            DebugCdpStatus {
                port,
                ok: true,
                target_count,
                browser,
                message: format!(
                    "CDP endpoint is reachable on port {port} with {} target(s)",
                    target_count.unwrap_or_default()
                ),
            }
        }
        Err(error) => DebugCdpStatus {
            port,
            ok: false,
            target_count: None,
            browser: None,
            message: format!("waiting for CDP endpoint on port {port}: {error}"),
        },
    }
}

fn get_cdp_json(port: u16, path: &str) -> Result<serde_json::Value> {
    let mut stream = TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse()?,
        Duration::from_millis(500),
    )?;
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = Vec::new();
    let mut byte = [0_u8; 1];
    while !response.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte)?;
        response.push(byte[0]);
    }
    let headers = String::from_utf8(response)?;
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .ok_or_else(|| anyhow!("CDP response did not include Content-Length"))?;
    let mut body = vec![0_u8; content_length];
    stream.read_exact(&mut body)?;
    Ok(serde_json::from_slice(&body)?)
}

fn readiness(stages: &[DebugStage], require_frontend: bool, require_drive_bridge: bool) -> bool {
    let required = [
        "app_discovery",
        "heartbeat",
        "telemetry_database",
        "session",
        "window",
        "backend_telemetry",
        "cdp_endpoint",
    ];
    required.iter().all(|name| {
        stages
            .iter()
            .find(|stage| stage.name == *name)
            .is_some_and(|stage| {
                stage.status == DebugStageStatus::Ok || stage.status == DebugStageStatus::Skipped
            })
    }) && (!require_frontend
        || stages
            .iter()
            .find(|stage| stage.name == "frontend_telemetry")
            .is_some_and(|stage| stage.status == DebugStageStatus::Ok))
        && (!require_drive_bridge
            || stages
                .iter()
                .find(|stage| stage.name == "drive_bridge")
                .is_some_and(|stage| stage.status == DebugStageStatus::Ok))
}

fn stage(
    name: impl Into<String>,
    status: DebugStageStatus,
    message: impl Into<String>,
) -> DebugStage {
    DebugStage {
        name: name.into(),
        status,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn startup_selectors_target_exact_session() {
        let app = DebugRunApp {
            service_name: "fixture".to_string(),
            session_id: "session-fixture".to_string(),
            instance_id: "instance-fixture".to_string(),
            pid: 42,
            database_path: "C:\\tmp\\telemetry.sqlite".to_string(),
            capabilities: vec!["logs".to_string()],
            pinned_by: "newDiscoveryAfterSpawn".to_string(),
        };

        let selectors = DebugRunSelectors::from_app(&app);

        assert_eq!(
            selectors.debug,
            vec![
                "--db",
                "C:\\tmp\\telemetry.sqlite",
                "--session-id",
                "session-fixture",
                "--instance-id",
                "instance-fixture",
                "--pid",
                "42"
            ]
        );
        assert_eq!(
            selectors.drive,
            vec![
                "--session-id",
                "session-fixture",
                "--instance-id",
                "instance-fixture",
                "--pid",
                "42"
            ]
        );
        assert_eq!(
            selectors.read,
            vec![
                "--db",
                "C:\\tmp\\telemetry.sqlite",
                "--session",
                "session-fixture"
            ]
        );
    }

    #[test]
    fn writes_startup_session_file() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join(".auditaur").join("session.json");
        let status = DebugStatus {
            schema_version: 1,
            generated_at_unix_nanos: 1,
            ready: true,
            app: None,
            database_path: Some("C:\\tmp\\telemetry.sqlite".to_string()),
            session_id: Some("session-fixture".to_string()),
            stages: Vec::new(),
            telemetry: DebugTelemetryCounts::default(),
            cdp: None,
            hints: Vec::new(),
        };
        let process = DebugProcessStatus {
            command: vec!["npm".to_string(), "run".to_string(), "debug".to_string()],
            pid: Some(123),
            exit_code: None,
            running: true,
            stdout_tail: None,
            stderr_tail: None,
        };
        let app = DiscoveredApp {
            instance_id: "instance-fixture".to_string(),
            session_id: "session-fixture".to_string(),
            service_name: "fixture".to_string(),
            service_version: None,
            app_identifier: None,
            pid: 42,
            started_at: "2099-01-01T00:00:00Z".to_string(),
            last_heartbeat_at: "2099-01-01T00:00:00Z".to_string(),
            heartbeat_age_seconds: Some(0),
            status: DiscoveryStatus::Active,
            capabilities: vec!["logs".to_string()],
            database_path: "C:\\tmp\\telemetry.sqlite".to_string(),
            database_readable: true,
            schema_valid: true,
            discovery_path: "C:\\tmp\\apps\\instance-fixture.json".to_string(),
            stale_reason: None,
            superseded_by_session_id: None,
            seconds_until_next_start: None,
            churn_session_count: None,
            churn_window_seconds: None,
            churn_hint: None,
        };
        let result = debug_run_result(status, process, Some(&app), &Some(path.clone()));

        write_session_file(Some(&path), &result).unwrap();

        let written: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(written["app"]["sessionId"], "session-fixture");
        assert_eq!(written["app"]["instanceId"], "instance-fixture");
        assert_eq!(written["selectors"]["read"][3], "session-fixture");
        assert_eq!(written["process"]["running"], true);
    }

    #[test]
    fn handoff_command_lines_use_pinned_selectors() {
        let app = DebugRunApp {
            service_name: "fixture".to_string(),
            session_id: "session fixture".to_string(),
            instance_id: "instance-fixture".to_string(),
            pid: 42,
            database_path: "C:\\tmp\\telemetry.sqlite".to_string(),
            capabilities: vec!["drive_bridge".to_string()],
            pinned_by: "newDiscoveryAfterSpawn".to_string(),
        };
        let selectors = DebugRunSelectors::from_app(&app);

        let status = command_line("auditaur", ["debug"], &selectors.debug, ["status"]);
        let tail = command_line("auditaur", ["tail"], &selectors.read, ["--replay"]);

        assert!(status.contains("--session-id \"session fixture\""));
        assert!(status.contains("--instance-id instance-fixture"));
        assert!(status.contains("--pid 42"));
        assert!(tail.contains("--session \"session fixture\""));
        assert!(!status.contains("--latest"));
    }

    #[test]
    fn process_status_can_include_captured_child_output() {
        let output = ChildOutputTail {
            stdout: Some("compile stdout".to_string()),
            stderr: Some("compile stderr".to_string()),
        };
        let process = DebugProcessStatus {
            command: vec!["npm".to_string()],
            pid: Some(123),
            exit_code: Some(1),
            running: false,
            stdout_tail: None,
            stderr_tail: None,
        }
        .with_output(&output);
        let serialized = serde_json::to_value(&process).unwrap();

        assert_eq!(serialized["stdoutTail"], "compile stdout");
        assert_eq!(serialized["stderrTail"], "compile stderr");
        assert!(format_child_output_for_error(&output).contains("compile stderr"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_executable_candidates_include_command_shims() {
        let candidates = executable_candidates(Path::new("npm"));

        assert!(candidates.iter().any(|candidate| {
            candidate
                .file_name()
                .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("npm.cmd"))
        }));
    }
}

fn print_status(status: &DebugStatus) -> Result<()> {
    let ok = status
        .stages
        .iter()
        .filter(|stage| stage.status == DebugStageStatus::Ok)
        .count();
    let waiting = status
        .stages
        .iter()
        .filter(|stage| stage.status == DebugStageStatus::Waiting)
        .count();
    let errors = status
        .stages
        .iter()
        .filter(|stage| stage.status == DebugStageStatus::Error)
        .count();
    println!(
        "Auditaur debug: {} (ok={} waiting={} errors={})",
        if status.ready { "ready" } else { "waiting" },
        ok,
        waiting,
        errors
    );
    for stage in &status.stages {
        println!(
            "{}\t{:?}\t{}",
            table_cell(&stage.name, 24),
            stage.status,
            table_cell(&stage.message, 96)
        );
    }
    println!(
        "telemetry\tsessions={} logs={} spans={} span_events={} frontend_errors={} ipc={} events={} windows={}",
        status.telemetry.sessions,
        status.telemetry.logs,
        status.telemetry.spans,
        status.telemetry.span_events,
        status.telemetry.frontend_errors,
        status.telemetry.ipc,
        status.telemetry.events,
        status.telemetry.windows
    );
    for hint in &status.hints {
        println!("hint\t{}", table_cell(hint, 120));
    }
    Ok(())
}
