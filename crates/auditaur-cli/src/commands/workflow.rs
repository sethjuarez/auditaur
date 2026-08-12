use std::{
    collections::BTreeMap,
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::commands::{debug, drill, drive, polish};

const DEFAULT_CONFIG_PATH: &str = ".auditaur/config.json";
const DEFAULT_SESSION_PATH: &str = ".auditaur/session.json";

#[derive(Debug)]
pub struct StartOptions {
    pub config: PathBuf,
    pub write_session: PathBuf,
    pub json: bool,
    pub command: Vec<String>,
}

#[derive(Debug)]
pub struct ObserveOptions {
    pub app: String,
    pub write_session: PathBuf,
    pub require_frontend: bool,
    pub require_drive_bridge: bool,
    pub timeout_seconds: u64,
    pub interval_ms: u64,
    pub ports: Vec<String>,
    pub port_env: Vec<String>,
    pub json: bool,
    pub command: Vec<String>,
}

#[derive(Debug)]
pub struct DrillOptions {
    pub config: PathBuf,
    pub session_file: PathBuf,
    pub name: Option<String>,
    pub json: bool,
}

#[derive(Debug)]
pub struct InspectOptions {
    pub session_file: PathBuf,
    pub json: bool,
    pub limit: usize,
}

#[derive(Debug)]
pub struct StopOptions {
    pub session_file: PathBuf,
    pub json: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AgentConfig {
    app: String,
    #[serde(default)]
    start: Option<CommandSpec>,
    #[serde(default)]
    readiness: ReadinessConfig,
    #[serde(default)]
    ports: BTreeMap<String, PortConfig>,
    #[serde(default)]
    default_drill: Option<String>,
    #[serde(default)]
    drills: BTreeMap<String, DrillConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PortConfig {
    #[serde(default)]
    env: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadinessConfig {
    #[serde(default)]
    frontend: bool,
    #[serde(default)]
    drive_bridge: bool,
    #[serde(default)]
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum CommandSpec {
    Shell(String),
    Argv(Vec<String>),
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DrillConfig {
    #[serde(default)]
    selector: Option<String>,
    #[serde(default)]
    expect_text: Option<String>,
    #[serde(default)]
    timeout_seconds: Option<u64>,
    #[serde(default)]
    require_frontend: Option<bool>,
    #[serde(default)]
    require_drive_bridge: Option<bool>,
    #[serde(default)]
    owned: bool,
}

#[derive(Debug, Clone)]
struct SessionFile {
    path: PathBuf,
    app: SessionApp,
    process: SessionProcess,
}

#[derive(Debug, Clone)]
struct SessionApp {
    service_name: String,
    session_id: String,
    instance_id: String,
    pid: u32,
    database_path: String,
}

#[derive(Debug, Clone)]
struct SessionProcess {
    pid: Option<u32>,
    running: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AttachedDrillReport {
    schema_version: u8,
    status: DrillStatus,
    app: AttachedDrillApp,
    drill: AttachedDrill,
    phases: Vec<AttachedDrillPhase>,
    inspect: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DrillStatus {
    Passed,
    Failed,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AttachedDrillApp {
    service_name: String,
    session_id: String,
    instance_id: String,
    pid: u32,
    database_path: String,
    session_file: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AttachedDrill {
    name: String,
    selector: Option<String>,
    expect_text: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AttachedDrillPhase {
    id: String,
    status: DrillStatus,
    message: String,
    result: Option<Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InspectReport {
    schema_version: u8,
    app: AttachedDrillApp,
    process: SessionProcessReport,
    explain: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionProcessReport {
    pid: Option<u32>,
    running: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StopReport {
    schema_version: u8,
    session_file: String,
    pid: Option<u32>,
    stopped: bool,
    message: String,
}

pub fn default_config_path() -> PathBuf {
    PathBuf::from(DEFAULT_CONFIG_PATH)
}

pub fn default_session_path() -> PathBuf {
    PathBuf::from(DEFAULT_SESSION_PATH)
}

pub fn start(options: StartOptions) -> Result<()> {
    let config = load_config(&options.config)?;
    let ports = allocate_ports(&config.ports)?;
    let command = if options.command.is_empty() {
        config
            .start
            .as_ref()
            .map(|command| command_spec_to_argv(command, &ports))
            .transpose()?
            .ok_or_else(|| {
                anyhow!(
                    "`{}` must define `start` or pass a command after `--`",
                    options.config.display()
                )
            })?
    } else {
        expand_argv(options.command, &ports)?
    };
    let environment = port_environment(&config.ports, &ports);
    let result = debug::run_with_output(
        debug::DebugSelector {
            db: None,
            app: Some(config.app),
            session_id: None,
            instance_id: None,
            pid: None,
            latest: false,
            active: false,
            cdp_port: None,
            require_frontend: config.readiness.frontend,
            require_drive_bridge: config.readiness.drive_bridge,
        },
        500,
        Some(config.readiness.timeout_seconds.unwrap_or(180)),
        if options.json {
            debug::DebugRunOutput::Quiet
        } else {
            debug::DebugRunOutput::Human
        },
        Some(options.write_session.clone()),
        environment,
        command,
        "Auditaur start session ready.",
    )?;
    write_ports_to_session_file(&options.write_session, &ports)?;
    if options.json {
        let mut value = serde_json::to_value(&result)?;
        if !ports.is_empty() {
            value["ports"] = json!(ports);
        }
        println!("{}", serde_json::to_string_pretty(&value)?);
    }
    Ok(())
}

pub fn observe(options: ObserveOptions) -> Result<()> {
    let (ports, environment) = observe_ports(&options.ports, &options.port_env)?;
    let command = expand_argv(options.command, &ports)?;
    let result = debug::run_with_output(
        debug::DebugSelector {
            db: None,
            app: Some(options.app),
            session_id: None,
            instance_id: None,
            pid: None,
            latest: false,
            active: false,
            cdp_port: None,
            require_frontend: options.require_frontend,
            require_drive_bridge: options.require_drive_bridge,
        },
        options.interval_ms,
        Some(options.timeout_seconds),
        if options.json {
            debug::DebugRunOutput::Quiet
        } else {
            debug::DebugRunOutput::Human
        },
        Some(options.write_session.clone()),
        environment,
        command,
        "Auditaur observe session ready.",
    )?;
    write_ports_to_session_file(&options.write_session, &ports)?;
    if options.json {
        let mut value = serde_json::to_value(&result)?;
        if !ports.is_empty() {
            value["ports"] = json!(ports);
        }
        println!("{}", serde_json::to_string_pretty(&value)?);
    }
    Ok(())
}

pub fn drill(options: DrillOptions) -> Result<()> {
    let config = load_config(&options.config)?;
    let (name, drill_config) = select_drill(&config, options.name.as_deref())?;
    if drill_config.owned || !options.session_file.is_file() {
        return run_owned_drill(&config, &name, &drill_config, options.json);
    }

    let session = read_session_file(&options.session_file)?;
    ensure_live_session(&session)?;
    let mut phases = Vec::new();
    let mut status = DrillStatus::Passed;
    if let Some(selector) = &drill_config.selector {
        let result = drive::text_json_value(
            drive_selector(&session),
            drive::SelectorActionOptions {
                selector: selector.clone(),
                target_id: Some("auditaur-bridge".to_string()),
                test_id: Some(format!("auditaur-drill-{name}")),
                step_id: Some("drive-text".to_string()),
                visible_only: true,
                json: true,
            },
        );
        match result {
            Ok(value) => {
                let text = value
                    .get("payload")
                    .and_then(|payload| payload.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if let Some(expected) = &drill_config.expect_text {
                    if !text.contains(expected) {
                        status = DrillStatus::Failed;
                        phases.push(AttachedDrillPhase {
                            id: "drive-text".to_string(),
                            status: DrillStatus::Failed,
                            message: format!(
                                "selector `{selector}` text did not contain expected text `{expected}`"
                            ),
                            result: Some(value),
                        });
                    } else {
                        phases.push(AttachedDrillPhase {
                            id: "drive-text".to_string(),
                            status: DrillStatus::Passed,
                            message: "selector text matched expected text".to_string(),
                            result: Some(value),
                        });
                    }
                } else {
                    phases.push(AttachedDrillPhase {
                        id: "drive-text".to_string(),
                        status: DrillStatus::Passed,
                        message: "selector text was readable".to_string(),
                        result: Some(value),
                    });
                }
            }
            Err(error) => {
                status = DrillStatus::Failed;
                phases.push(AttachedDrillPhase {
                    id: "drive-text".to_string(),
                    status: DrillStatus::Failed,
                    message: error.to_string(),
                    result: None,
                });
            }
        }
    } else {
        phases.push(AttachedDrillPhase {
            id: "configured-check".to_string(),
            status: DrillStatus::Passed,
            message: "no selector check configured".to_string(),
            result: None,
        });
    }

    let inspect = polish::explain_json_value(
        &Some(PathBuf::from(&session.app.database_path)),
        Some(session.app.session_id.clone()),
        None,
        None,
        200,
    )
    .unwrap_or_else(|error| json!({ "error": error.to_string() }));
    let report = AttachedDrillReport {
        schema_version: 1,
        status,
        app: attached_app(&session),
        drill: AttachedDrill {
            name,
            selector: drill_config.selector,
            expect_text: drill_config.expect_text,
        },
        phases,
        inspect,
    };
    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Auditaur drill {:?}: {}", report.status, report.drill.name);
    }
    if status == DrillStatus::Passed {
        Ok(())
    } else {
        Err(anyhow!("Auditaur drill failed"))
    }
}

pub fn inspect(options: InspectOptions) -> Result<()> {
    let session = read_session_file(&options.session_file)?;
    ensure_live_session(&session)?;
    let explain = polish::explain_json_value(
        &Some(PathBuf::from(&session.app.database_path)),
        Some(session.app.session_id.clone()),
        None,
        None,
        options.limit,
    )?;
    let report = InspectReport {
        schema_version: 1,
        app: attached_app(&session),
        process: SessionProcessReport {
            pid: session.process.pid,
            running: session.process.running,
        },
        explain,
    };
    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "Auditaur inspect: {} session {}",
            report.app.service_name, report.app.session_id
        );
        if let Some(findings) = report.explain.get("findings").and_then(Value::as_array) {
            for finding in findings {
                if let Some(finding) = finding.as_str() {
                    println!("- {finding}");
                }
            }
        }
    }
    Ok(())
}

pub fn stop(options: StopOptions) -> Result<()> {
    let session = read_session_file(&options.session_file)?;
    let pid = session.process.pid.or(Some(session.app.pid));
    let Some(pid) = pid else {
        return Err(anyhow!(
            "`{}` does not include a process pid to stop",
            options.session_file.display()
        ));
    };
    if !session.process.running {
        remove_session_file(&options.session_file)?;
        return Err(anyhow!(
            "`{}` is already marked as stopped; removed stale session file",
            options.session_file.display()
        ));
    }
    if !process_exists(pid)? {
        remove_session_file(&options.session_file)?;
        return Err(anyhow!(
            "`{}` references pid {pid}, but that process is no longer running; removed stale session file",
            options.session_file.display()
        ));
    }
    let message = cleanup_process_tree(pid)?;
    remove_session_file(&options.session_file)?;
    let report = StopReport {
        schema_version: 1,
        session_file: options.session_file.to_string_lossy().to_string(),
        pid: Some(pid),
        stopped: true,
        message,
    };
    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("{}", report.message);
    }
    Ok(())
}

fn load_config(path: &Path) -> Result<AgentConfig> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read `{}`", path.display()))?;
    serde_json::from_str(&contents).with_context(|| format!("failed to parse `{}`", path.display()))
}

fn command_spec_to_argv(
    command: &CommandSpec,
    ports: &BTreeMap<String, u16>,
) -> Result<Vec<String>> {
    match command {
        CommandSpec::Argv(argv) if argv.is_empty() => {
            Err(anyhow!("start command must not be empty"))
        }
        CommandSpec::Argv(argv) => expand_argv(argv.clone(), ports),
        CommandSpec::Shell(command) if command.trim().is_empty() => {
            Err(anyhow!("start command must not be empty"))
        }
        CommandSpec::Shell(command) => {
            let command = expand_template(command, ports)?;
            #[cfg(windows)]
            {
                Ok(vec!["cmd".to_string(), "/C".to_string(), command])
            }
            #[cfg(not(windows))]
            {
                Ok(vec!["sh".to_string(), "-c".to_string(), command])
            }
        }
    }
}

fn allocate_ports(config: &BTreeMap<String, PortConfig>) -> Result<BTreeMap<String, u16>> {
    config
        .keys()
        .map(|name| {
            let listener = TcpListener::bind("127.0.0.1:0")
                .with_context(|| format!("failed to reserve random port for `{name}`"))?;
            let port = listener
                .local_addr()
                .context("failed to read reserved port")?
                .port();
            drop(listener);
            Ok((name.clone(), port))
        })
        .collect()
}

fn observe_ports(
    port_specs: &[String],
    env_specs: &[String],
) -> Result<(BTreeMap<String, u16>, Vec<(String, String)>)> {
    let mut requested = BTreeMap::new();
    for spec in port_specs {
        let (name, port) = parse_observe_port(spec)?;
        if requested.insert(name.clone(), port).is_some() {
            return Err(anyhow!("duplicate observe port `{name}`"));
        }
    }

    let env = parse_observe_port_env(env_specs)?;
    for name in env.keys() {
        requested.entry(name.clone()).or_insert(None);
    }

    let ports = requested
        .iter()
        .map(|(name, port)| {
            let port = match port {
                Some(port) => reserve_specific_port(*port).with_context(|| {
                    format!("port `{name}` requested {port}, but it is not available")
                })?,
                None => reserve_random_port()
                    .with_context(|| format!("failed to reserve random port for `{name}`"))?,
            };
            Ok((name.clone(), port))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let environment = env
        .into_iter()
        .filter_map(|(name, env)| {
            let port = ports.get(&name)?;
            Some((env, port.to_string()))
        })
        .collect();
    Ok((ports, environment))
}

fn parse_observe_port(spec: &str) -> Result<(String, Option<u16>)> {
    let (name, port) = match spec.split_once('=') {
        Some((name, port)) => (name, Some(parse_port(port)?)),
        None => (spec, None),
    };
    let name = validate_port_name(name)?;
    Ok((name, port))
}

fn parse_observe_port_env(spec: &[String]) -> Result<BTreeMap<String, String>> {
    let mut env = BTreeMap::new();
    for spec in spec {
        let (name, env_name) = spec
            .split_once('=')
            .ok_or_else(|| anyhow!("--port-env expects NAME=ENV"))?;
        let name = validate_port_name(name)?;
        if env_name.trim().is_empty() {
            return Err(anyhow!(
                "--port-env for `{name}` must include an environment variable name"
            ));
        }
        if env.insert(name.clone(), env_name.to_string()).is_some() {
            return Err(anyhow!("duplicate observe port environment for `{name}`"));
        }
    }
    Ok(env)
}

fn validate_port_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(anyhow!("observe port name must not be empty"));
    }
    if !name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(anyhow!(
            "observe port name `{name}` must contain only ASCII letters, numbers, '-' or '_'"
        ));
    }
    Ok(name.to_string())
}

fn parse_port(port: &str) -> Result<u16> {
    let port = port
        .parse::<u16>()
        .with_context(|| format!("invalid port `{port}`"))?;
    if port == 0 {
        return Err(anyhow!(
            "port 0 is not valid; omit =PORT to reserve a random port"
        ));
    }
    Ok(port)
}

fn reserve_specific_port(port: u16) -> Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .with_context(|| format!("failed to reserve port {port}"))?;
    drop(listener);
    Ok(port)
}

fn reserve_random_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").context("failed to reserve random port")?;
    let port = listener
        .local_addr()
        .context("failed to read reserved port")?
        .port();
    drop(listener);
    Ok(port)
}

fn port_environment(
    config: &BTreeMap<String, PortConfig>,
    ports: &BTreeMap<String, u16>,
) -> Vec<(String, String)> {
    config
        .iter()
        .filter_map(|(name, config)| {
            let env = config.env.as_ref()?;
            let port = ports.get(name)?;
            Some((env.clone(), port.to_string()))
        })
        .collect()
}

fn expand_argv(argv: Vec<String>, ports: &BTreeMap<String, u16>) -> Result<Vec<String>> {
    argv.into_iter()
        .map(|arg| expand_template(&arg, ports))
        .collect()
}

fn expand_template(value: &str, ports: &BTreeMap<String, u16>) -> Result<String> {
    let mut expanded = value.to_string();
    for (name, port) in ports {
        expanded = expanded.replace(&format!("{{{{port:{name}}}}}"), &port.to_string());
    }
    if let Some(start) = expanded.find("{{port:") {
        let tail = &expanded[start..];
        let end = tail.find("}}").unwrap_or(tail.len());
        return Err(anyhow!(
            "unknown port placeholder `{}`",
            &tail[..end.saturating_add(2).min(tail.len())]
        ));
    }
    Ok(expanded)
}

fn write_ports_to_session_file(path: &Path, ports: &BTreeMap<String, u16>) -> Result<()> {
    if ports.is_empty() {
        return Ok(());
    }
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read `{}`", path.display()))?;
    let mut value: Value = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse `{}`", path.display()))?;
    value["ports"] = json!(ports);
    fs::write(path, serde_json::to_vec_pretty(&value)?)
        .with_context(|| format!("failed to write `{}`", path.display()))
}

fn select_drill(config: &AgentConfig, requested: Option<&str>) -> Result<(String, DrillConfig)> {
    let name = requested
        .map(str::to_string)
        .or_else(|| config.default_drill.clone())
        .or_else(|| {
            (config.drills.len() == 1)
                .then(|| config.drills.keys().next().cloned())
                .flatten()
        })
        .ok_or_else(|| {
            anyhow!(
                "`defaultDrill` is required when `{}` contains multiple or zero drills",
                DEFAULT_CONFIG_PATH
            )
        })?;
    let drill =
        config.drills.get(&name).cloned().ok_or_else(|| {
            anyhow!("No drill named `{name}` was found in `{DEFAULT_CONFIG_PATH}`")
        })?;
    Ok((name, drill))
}

fn run_owned_drill(
    config: &AgentConfig,
    name: &str,
    drill_config: &DrillConfig,
    json: bool,
) -> Result<()> {
    let ports = allocate_ports(&config.ports)?;
    let command = config
        .start
        .as_ref()
        .map(|command| command_spec_to_argv(command, &ports))
        .transpose()?
        .ok_or_else(|| {
            anyhow!("owned drill `{name}` requires `start` in `{DEFAULT_CONFIG_PATH}`")
        })?;
    let environment = port_environment(&config.ports, &ports);
    let exit_code = drill::run(drill::DrillRunOptions {
        app: config.app.clone(),
        require_frontend: drill_config
            .require_frontend
            .unwrap_or(config.readiness.frontend),
        require_drive_bridge: drill_config
            .require_drive_bridge
            .unwrap_or(config.readiness.drive_bridge),
        timeout_seconds: drill_config
            .timeout_seconds
            .or(config.readiness.timeout_seconds)
            .unwrap_or(180),
        interval_ms: 500,
        report: PathBuf::from(format!(".auditaur/{name}-drill-report.json")),
        selector: drill_config.selector.clone(),
        expect_text: drill_config.expect_text.clone(),
        script: None,
        json,
        environment,
        command,
    })?;
    if exit_code == 0 {
        Ok(())
    } else {
        Err(anyhow!(
            "Auditaur owned drill failed with exit code {exit_code}"
        ))
    }
}

fn read_session_file(path: &Path) -> Result<SessionFile> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read `{}`", path.display()))?;
    let value: Value = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse `{}`", path.display()))?;
    let app = value
        .get("app")
        .ok_or_else(|| anyhow!("`{}` is missing app", path.display()))?;
    let process = value.get("process").unwrap_or(&Value::Null);
    Ok(SessionFile {
        path: path.to_path_buf(),
        app: SessionApp {
            service_name: required_string(app, "serviceName")?,
            session_id: required_string(app, "sessionId")?,
            instance_id: required_string(app, "instanceId")?,
            pid: required_u32(app, "pid")?,
            database_path: required_string(app, "databasePath")?,
        },
        process: SessionProcess {
            pid: optional_u32(process, "pid")?,
            running: process
                .get("running")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        },
    })
}

fn ensure_live_session(session: &SessionFile) -> Result<debug::DebugStatus> {
    if !session.process.running {
        return Err(anyhow!(
            "`{}` is marked as stopped; run `auditaur start` before using it",
            session.path.display()
        ));
    }
    let pid = session.process.pid.unwrap_or(session.app.pid);
    if !process_exists(pid)? {
        return Err(anyhow!(
            "`{}` references pid {pid}, but that process is no longer running; run `auditaur start` again",
            session.path.display()
        ));
    }
    let status = debug::snapshot(&debug_selector(session)).with_context(|| {
        format!(
            "failed to read Auditaur status for session {}",
            session.app.session_id
        )
    })?;
    if !status.ready {
        return Err(anyhow!(
            "`{}` does not describe a ready live Auditaur session; run `auditaur start` again",
            session.path.display()
        ));
    }
    Ok(status)
}

fn debug_selector(session: &SessionFile) -> debug::DebugSelector {
    debug::DebugSelector {
        db: Some(PathBuf::from(&session.app.database_path)),
        app: None,
        session_id: Some(session.app.session_id.clone()),
        instance_id: Some(session.app.instance_id.clone()),
        pid: Some(session.app.pid),
        latest: false,
        active: false,
        cdp_port: None,
        require_frontend: false,
        require_drive_bridge: false,
    }
}

fn remove_session_file(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to remove `{}`", path.display())),
    }
}

#[cfg(windows)]
fn process_exists(pid: u32) -> Result<bool> {
    let output = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .context("failed to invoke tasklist")?;
    if !output.status.success() {
        return Err(anyhow!(
            "tasklist failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).contains(&format!("\",\"{pid}\",")))
}

#[cfg(not(windows))]
fn process_exists(pid: u32) -> Result<bool> {
    let status = Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to invoke kill")?;
    Ok(status.success())
}

fn required_string(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("session file app.{key} must be a string"))
}

fn required_u32(value: &Value, key: &str) -> Result<u32> {
    let raw = value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("session file app.{key} must be a number"))?;
    u32::try_from(raw).map_err(|_| anyhow!("session file app.{key} is out of range"))
}

fn optional_u32(value: &Value, key: &str) -> Result<Option<u32>> {
    let Some(raw) = value.get(key) else {
        return Ok(None);
    };
    if raw.is_null() {
        return Ok(None);
    }
    let value = raw
        .as_u64()
        .ok_or_else(|| anyhow!("session file process.{key} must be a number"))?;
    u32::try_from(value)
        .map(Some)
        .map_err(|_| anyhow!("session file process.{key} is out of range"))
}

fn attached_app(session: &SessionFile) -> AttachedDrillApp {
    AttachedDrillApp {
        service_name: session.app.service_name.clone(),
        session_id: session.app.session_id.clone(),
        instance_id: session.app.instance_id.clone(),
        pid: session.app.pid,
        database_path: session.app.database_path.clone(),
        session_file: session.path.to_string_lossy().to_string(),
    }
}

fn drive_selector(session: &SessionFile) -> drive::DriveAppSelector {
    drive::DriveAppSelector {
        app: None,
        session_id: Some(session.app.session_id.clone()),
        instance_id: Some(session.app.instance_id.clone()),
        pid: Some(session.app.pid),
        latest: false,
        active: false,
    }
}

#[cfg(windows)]
fn cleanup_process_tree(pid: u32) -> Result<String> {
    let output = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .context("failed to invoke taskkill")?;
    if output.status.success() {
        Ok(format!("stopped process tree rooted at pid {pid}"))
    } else {
        Err(anyhow!(
            "taskkill failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(not(windows))]
fn cleanup_process_tree(pid: u32) -> Result<String> {
    let process_group = format!("-{pid}");
    let status = Command::new("kill")
        .args(["-TERM", &process_group])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .context("failed to invoke kill")?;
    if status.success() {
        Ok(format!("stopped process tree rooted at pid {pid}"))
    } else {
        Err(anyhow!("kill failed for process group {pid}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_single_drill_as_default() {
        let config: AgentConfig = serde_json::from_str(
            r##"{
              "app": "fixture",
              "start": ["npm", "run", "debug"],
              "drills": {
                "smoke": { "selector": "#ready", "expectText": "Ready" }
              }
            }"##,
        )
        .unwrap();

        let (name, drill) = select_drill(&config, None).unwrap();

        assert_eq!(name, "smoke");
        assert_eq!(drill.selector.as_deref(), Some("#ready"));
    }

    #[test]
    fn parses_shell_start_command() {
        let command = command_spec_to_argv(
            &CommandSpec::Shell("npm run debug".to_string()),
            &BTreeMap::new(),
        )
        .unwrap();

        #[cfg(windows)]
        assert_eq!(command, vec!["cmd", "/C", "npm run debug"]);
        #[cfg(not(windows))]
        assert_eq!(command, vec!["sh", "-c", "npm run debug"]);
    }

    #[test]
    fn expands_named_port_placeholders() {
        let mut ports = BTreeMap::new();
        ports.insert("web".to_string(), 1437);

        let command = command_spec_to_argv(
            &CommandSpec::Shell("npm run dev -- --port {{port:web}}".to_string()),
            &ports,
        )
        .unwrap();

        assert!(command.last().unwrap().contains("--port 1437"));
    }

    #[test]
    fn configured_ports_expand_command_and_environment() {
        let config: AgentConfig = serde_json::from_str(
            r##"{
              "app": "fixture",
              "start": "npm run dev -- --port {{port:web}}",
              "ports": {
                "web": { "env": "AUDITAUR_WEB_PORT" }
              },
              "drills": {
                "smoke": { "selector": "#ready" }
              }
            }"##,
        )
        .unwrap();
        let mut ports = BTreeMap::new();
        ports.insert("web".to_string(), 4242);

        let command = command_spec_to_argv(config.start.as_ref().unwrap(), &ports).unwrap();
        let environment = port_environment(&config.ports, &ports);

        assert!(command.last().unwrap().contains("--port 4242"));
        assert_eq!(
            environment,
            vec![("AUDITAUR_WEB_PORT".to_string(), "4242".to_string())]
        );
    }

    #[test]
    fn observe_ports_allocate_random_ports_and_environment() {
        let (ports, environment) = observe_ports(
            &["web".to_string()],
            &["web=VITE_PORT".to_string(), "api=API_PORT".to_string()],
        )
        .unwrap();

        assert!(ports.get("web").is_some_and(|port| *port > 0));
        assert!(ports.get("api").is_some_and(|port| *port > 0));
        assert_eq!(
            environment,
            vec![
                ("API_PORT".to_string(), ports["api"].to_string()),
                ("VITE_PORT".to_string(), ports["web"].to_string())
            ]
        );
        let command = expand_argv(
            vec![
                "npm".to_string(),
                "run".to_string(),
                "dev".to_string(),
                "--".to_string(),
                "--port".to_string(),
                "{{port:web}}".to_string(),
            ],
            &ports,
        )
        .unwrap();
        assert_eq!(command.last().unwrap(), &ports["web"].to_string());
    }

    #[test]
    fn observe_ports_accept_explicit_port_values() {
        let random = reserve_random_port().unwrap();
        let (ports, _) = observe_ports(&[format!("web={random}")], &[]).unwrap();

        assert_eq!(ports["web"], random);
    }

    #[test]
    fn stopped_session_files_are_removed() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("session.json");
        fs::write(&path, "{}").unwrap();

        remove_session_file(&path).unwrap();
        remove_session_file(&path).unwrap();

        assert!(!path.exists());
    }

    #[test]
    fn stopped_session_is_not_considered_live() {
        let session = SessionFile {
            path: PathBuf::from(".auditaur/session.json"),
            app: SessionApp {
                service_name: "fixture".to_string(),
                session_id: "session".to_string(),
                instance_id: "instance".to_string(),
                pid: 123,
                database_path: "telemetry.sqlite".to_string(),
            },
            process: SessionProcess {
                pid: Some(123),
                running: false,
            },
        };

        let error = ensure_live_session(&session).unwrap_err();

        assert!(error.to_string().contains("marked as stopped"));
    }
}
