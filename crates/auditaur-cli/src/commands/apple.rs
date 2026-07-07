use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use time::OffsetDateTime;

use crate::commands::read::print_json_or_table;

#[derive(Debug, Clone)]
pub struct ObserveOptions {
    pub scheme: Option<String>,
    pub destination: String,
    pub workspace: Option<PathBuf>,
    pub project: Option<PathBuf>,
    pub bundle_id: Option<String>,
    pub app_path: Option<PathBuf>,
    pub screenshot: Option<PathBuf>,
    pub report: PathBuf,
    pub diagnostics: Option<PathBuf>,
    pub log_predicate: Option<String>,
    pub log_seconds: u64,
    pub skip_build: bool,
    pub skip_launch: bool,
    pub json: bool,
}

#[derive(Debug, Clone)]
pub struct ScreenshotOptions {
    pub destination: String,
    pub output: PathBuf,
    pub json: bool,
}

#[derive(Debug, Clone)]
pub struct LogsOptions {
    pub destination: String,
    pub output: Option<PathBuf>,
    pub predicate: Option<String>,
    pub seconds: u64,
    pub json: bool,
}

#[derive(Debug, Clone)]
pub struct StatusOptions {
    pub destination: String,
    pub json: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppleObserveReport {
    pub schema_version: u32,
    pub generated_at: String,
    pub destination: String,
    pub simulator: SimulatorSelection,
    pub build: AppleStepReport,
    pub install: AppleStepReport,
    pub launch: AppleStepReport,
    pub screenshot: AppleStepReport,
    pub logs: AppleLogSummary,
    pub diagnostics: AppleDiagnosticsSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SimulatorSelection {
    pub name: String,
    pub udid: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppleStepReport {
    pub status: AppleStepStatus,
    pub command: Vec<String>,
    pub path: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AppleStepStatus {
    Passed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppleLogSummary {
    pub status: AppleStepStatus,
    pub command: Vec<String>,
    pub output_path: Option<String>,
    pub line_count: usize,
    pub error_count: usize,
    pub recent_errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppleDiagnosticsSummary {
    pub status: AppleStepStatus,
    pub source_path: Option<String>,
    pub batch_count: usize,
    pub event_count: usize,
    pub span_count: usize,
    pub span_event_count: usize,
    pub error_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SimctlDevices {
    devices: std::collections::BTreeMap<String, Vec<SimctlDevice>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SimctlDevice {
    name: String,
    udid: String,
    state: String,
    #[serde(default)]
    is_available: bool,
}

pub fn observe(options: ObserveOptions) -> Result<()> {
    let simulator = resolve_and_boot_destination(&options.destination)?;

    let build = if options.skip_build {
        skipped_step("build", build_command(&options))
    } else if options.scheme.is_some() {
        run_step("build", build_command(&options))?
    } else {
        skipped_step("build", Vec::new()).with_detail("pass --scheme to run xcodebuild")
    };

    let install = if let Some(app_path) = options.app_path.as_ref() {
        run_step(
            "install",
            simctl_command([
                "install".into(),
                simulator.udid.clone().into(),
                app_path.into(),
            ]),
        )?
        .with_path(app_path)
    } else {
        skipped_step("install", Vec::new()).with_detail("pass --app-path to install a built .app")
    };

    let launch = if options.skip_launch {
        skipped_step(
            "launch",
            launch_command(&simulator.udid, options.bundle_id.as_deref()),
        )
    } else if let Some(bundle_id) = options.bundle_id.as_deref() {
        run_step("launch", launch_command(&simulator.udid, Some(bundle_id)))?
    } else {
        skipped_step("launch", Vec::new()).with_detail("pass --bundle-id to launch the app")
    };

    let screenshot = if let Some(path) = options.screenshot.as_ref() {
        ensure_parent(path)?;
        run_step("screenshot", screenshot_command(&simulator.udid, path))?.with_path(path)
    } else {
        skipped_step("screenshot", Vec::new()).with_detail("pass --screenshot to capture pixels")
    };

    let logs = collect_logs(
        &simulator.udid,
        &options.log_predicate,
        options.log_seconds,
        None,
    )?;
    let diagnostics = collect_diagnostics(options.diagnostics.as_deref())?;
    let report = AppleObserveReport {
        schema_version: 1,
        generated_at: OffsetDateTime::now_utc().to_string(),
        destination: options.destination,
        simulator,
        build,
        install,
        launch,
        screenshot,
        logs,
        diagnostics,
    };
    write_report(&options.report, &report)?;
    print_json_or_table(options.json, &report, || {
        println!(
            "Apple observe report: {} (simulator: {}, diagnostics: {} events, {} spans, {} errors)",
            options.report.display(),
            report.simulator.name,
            report.diagnostics.event_count,
            report.diagnostics.span_count,
            report.diagnostics.error_count
        );
        Ok(())
    })
}

pub fn screenshot(options: ScreenshotOptions) -> Result<()> {
    let simulator = resolve_and_boot_destination(&options.destination)?;
    ensure_parent(&options.output)?;
    let report = run_step(
        "screenshot",
        screenshot_command(&simulator.udid, &options.output),
    )?
    .with_path(&options.output);
    print_json_or_table(options.json, &report, || {
        println!("Screenshot written: {}", options.output.display());
        Ok(())
    })
}

pub fn logs(options: LogsOptions) -> Result<()> {
    let simulator = resolve_and_boot_destination(&options.destination)?;
    let summary = collect_logs(
        &simulator.udid,
        &options.predicate,
        options.seconds,
        options.output.as_deref(),
    )?;
    print_json_or_table(options.json, &summary, || {
        println!(
            "Collected {} log lines ({} errors)",
            summary.line_count, summary.error_count
        );
        Ok(())
    })
}

pub fn status(options: StatusOptions) -> Result<()> {
    let simulator = resolve_destination(&options.destination)?;
    print_json_or_table(options.json, &simulator, || {
        println!(
            "{} {} ({})",
            simulator.name, simulator.udid, simulator.state
        );
        Ok(())
    })
}

fn resolve_and_boot_destination(destination: &str) -> Result<SimulatorSelection> {
    let simulator = resolve_destination(destination)?;
    if simulator.state != "Booted" {
        run_command(&simctl_command([
            "boot".into(),
            simulator.udid.clone().into(),
        ]))?;
    }
    run_command(&simctl_command([
        "bootstatus".into(),
        simulator.udid.clone().into(),
        "-b".into(),
    ]))?;
    Ok(SimulatorSelection {
        state: "Booted".to_string(),
        ..simulator
    })
}

fn resolve_destination(destination: &str) -> Result<SimulatorSelection> {
    let devices = run_command_capture(&simctl_command([
        "list".into(),
        "devices".into(),
        "--json".into(),
    ]))?;
    select_simulator(destination, &devices)
}

fn select_simulator(destination: &str, simctl_devices_json: &str) -> Result<SimulatorSelection> {
    let selector = DestinationSelector::parse(destination);
    let devices: SimctlDevices = serde_json::from_str(simctl_devices_json)
        .context("failed to parse `xcrun simctl list devices --json` output")?;
    if let Some(udid) = selector.udid.as_deref() {
        let device = devices
            .devices
            .values()
            .flat_map(|devices| devices.iter())
            .find(|device| device.udid == udid)
            .ok_or_else(|| anyhow!("no iOS Simulator with UDID `{udid}` was found"))?;
        return Ok(SimulatorSelection {
            name: device.name.clone(),
            udid: device.udid.clone(),
            state: device.state.clone(),
        });
    }
    let requested_name = selector.name.as_deref().unwrap_or(destination);
    let candidates: Vec<(&String, &SimctlDevice)> = devices
        .devices
        .iter()
        .filter(|(runtime, _)| selector.matches_runtime(runtime))
        .flat_map(|(runtime, devices)| devices.iter().map(move |device| (runtime, device)))
        .filter(|(_, device)| device.name == requested_name)
        .collect();
    let device = candidates
        .iter()
        .find(|(_, device)| device.is_available)
        .map(|(_, device)| *device)
        .or_else(|| candidates.iter().map(|(_, device)| *device).next())
        .ok_or_else(|| anyhow!("no iOS Simulator named `{requested_name}` was found"))?;
    Ok(SimulatorSelection {
        name: device.name.clone(),
        udid: device.udid.clone(),
        state: device.state.clone(),
    })
}

#[derive(Debug, Default, PartialEq, Eq)]
struct DestinationSelector {
    name: Option<String>,
    udid: Option<String>,
    os: Option<String>,
}

impl DestinationSelector {
    fn parse(destination: &str) -> Self {
        if !destination.contains('=') {
            return Self {
                name: Some(destination.to_string()),
                ..Self::default()
            };
        }
        let mut selector = Self::default();
        for part in destination.split(',') {
            let Some((key, value)) = part.trim().split_once('=') else {
                continue;
            };
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            match key.trim().to_ascii_lowercase().as_str() {
                "name" => selector.name = Some(value.to_string()),
                "id" => selector.udid = Some(value.to_string()),
                "os" => selector.os = Some(value.to_string()),
                _ => {}
            }
        }
        selector
    }

    fn matches_runtime(&self, runtime: &str) -> bool {
        match self.os.as_deref() {
            None | Some("latest") => true,
            Some(os) => runtime.contains(os) || runtime.contains(&os.replace('.', "-")),
        }
    }
}

fn build_command(options: &ObserveOptions) -> Vec<String> {
    let Some(scheme) = options.scheme.as_ref() else {
        return Vec::new();
    };
    let mut command = vec!["xcodebuild".to_string(), "build".to_string()];
    if let Some(workspace) = options.workspace.as_ref() {
        command.push("-workspace".to_string());
        command.push(workspace.display().to_string());
    }
    if let Some(project) = options.project.as_ref() {
        command.push("-project".to_string());
        command.push(project.display().to_string());
    }
    command.push("-scheme".to_string());
    command.push(scheme.clone());
    command.push("-destination".to_string());
    command.push(options.destination.clone());
    command
}

fn launch_command(udid: &str, bundle_id: Option<&str>) -> Vec<String> {
    let Some(bundle_id) = bundle_id else {
        return Vec::new();
    };
    simctl_command(["launch".into(), udid.into(), bundle_id.into()])
}

fn screenshot_command(udid: &str, path: &Path) -> Vec<String> {
    simctl_command([
        "io".into(),
        udid.into(),
        "screenshot".into(),
        path.display().to_string().into(),
    ])
}

fn log_command(udid: &str, predicate: &Option<String>, seconds: u64) -> Vec<String> {
    let mut command = simctl_command([
        "spawn".into(),
        udid.into(),
        "log".into(),
        "show".into(),
        "--style".into(),
        "json".into(),
        "--last".into(),
        format!("{seconds}s").into(),
    ]);
    if let Some(predicate) = predicate {
        command.push("--predicate".to_string());
        command.push(predicate.clone());
    }
    command
}

fn simctl_command<I>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut command = vec!["xcrun".to_string(), "simctl".to_string()];
    command.extend(
        args.into_iter()
            .map(|arg| arg.to_string_lossy().to_string()),
    );
    command
}

fn collect_logs(
    udid: &str,
    predicate: &Option<String>,
    seconds: u64,
    output: Option<&Path>,
) -> Result<AppleLogSummary> {
    let command = log_command(udid, predicate, seconds);
    let stdout = run_command_capture(&command)?;
    if let Some(output) = output {
        ensure_parent(output)?;
        fs::write(output, &stdout)
            .with_context(|| format!("failed to write logs to `{}`", output.display()))?;
    }
    Ok(summarize_logs(&stdout, command, output))
}

fn summarize_logs(logs: &str, command: Vec<String>, output: Option<&Path>) -> AppleLogSummary {
    let lines: Vec<&str> = logs
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let mut error_count = 0;
    let mut recent_errors = Vec::new();
    for line in lines.iter().rev() {
        let is_error = line.contains("\"ERROR\"")
            || line.contains("\"FAULT\"")
            || line.to_ascii_lowercase().contains("error");
        if is_error {
            error_count += 1;
            if recent_errors.len() < 10 {
                recent_errors.push((*line).to_string());
            }
        }
    }
    recent_errors.reverse();
    AppleLogSummary {
        status: AppleStepStatus::Passed,
        command,
        output_path: output.map(|path| path.display().to_string()),
        line_count: lines.len(),
        error_count,
        recent_errors,
    }
}

fn collect_diagnostics(path: Option<&Path>) -> Result<AppleDiagnosticsSummary> {
    let Some(path) = path else {
        return Ok(AppleDiagnosticsSummary {
            status: AppleStepStatus::Skipped,
            source_path: None,
            batch_count: 0,
            event_count: 0,
            span_count: 0,
            span_event_count: 0,
            error_count: 0,
        });
    };
    let mut values = Vec::new();
    if path.is_dir() {
        for entry in
            fs::read_dir(path).with_context(|| format!("failed to read `{}`", path.display()))?
        {
            let entry = entry?;
            if entry.path().extension().and_then(|value| value.to_str()) == Some("json") {
                values.push(read_json_file(&entry.path())?);
            }
        }
    } else {
        values.push(read_json_file(path)?);
    }
    Ok(summarize_diagnostics(path, values))
}

fn read_json_file(path: &Path) -> Result<Value> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read diagnostics `{}`", path.display()))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse diagnostics `{}`", path.display()))
}

fn summarize_diagnostics(path: &Path, values: Vec<Value>) -> AppleDiagnosticsSummary {
    let mut summary = AppleDiagnosticsSummary {
        status: AppleStepStatus::Passed,
        source_path: Some(path.display().to_string()),
        batch_count: 0,
        event_count: 0,
        span_count: 0,
        span_event_count: 0,
        error_count: 0,
    };
    for value in values {
        add_diagnostics_value(&mut summary, &value);
    }
    summary
}

fn add_diagnostics_value(summary: &mut AppleDiagnosticsSummary, value: &Value) {
    if let Some(items) = value.as_array() {
        for item in items {
            add_diagnostics_value(summary, item);
        }
        return;
    }
    if !value.is_object() {
        return;
    }
    summary.batch_count += 1;
    summary.event_count += array_len(value, "events");
    summary.span_count += array_len(value, "spans");
    summary.span_event_count += array_len(value, "spanEvents");
    summary.error_count += array_len(value, "errors");
}

fn array_len(value: &Value, key: &str) -> usize {
    value.get(key).and_then(Value::as_array).map_or(0, Vec::len)
}

fn run_step(name: &str, command: Vec<String>) -> Result<AppleStepReport> {
    run_command(&command).with_context(|| format!("Apple {name} command failed"))?;
    Ok(AppleStepReport {
        status: AppleStepStatus::Passed,
        command,
        path: None,
        detail: None,
    })
}

fn skipped_step(_name: &str, command: Vec<String>) -> AppleStepReport {
    AppleStepReport {
        status: AppleStepStatus::Skipped,
        command,
        path: None,
        detail: None,
    }
}

impl AppleStepReport {
    fn with_path(mut self, path: &Path) -> Self {
        self.path = Some(path.display().to_string());
        self
    }

    fn with_detail(mut self, detail: &str) -> Self {
        self.detail = Some(detail.to_string());
        self
    }
}

fn run_command(command: &[String]) -> Result<()> {
    if command.is_empty() {
        return Ok(());
    }
    let status = Command::new(&command[0])
        .args(&command[1..])
        .status()
        .with_context(|| format!("failed to start `{}`", command.join(" ")))?;
    if !status.success() {
        return Err(anyhow!("`{}` exited with {status}", command.join(" ")));
    }
    Ok(())
}

fn run_command_capture(command: &[String]) -> Result<String> {
    let output = Command::new(&command[0])
        .args(&command[1..])
        .output()
        .with_context(|| format!("failed to start `{}`", command.join(" ")))?;
    if !output.status.success() {
        return Err(anyhow!(
            "`{}` exited with {}\n{}",
            command.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn write_report(path: &Path, report: &AppleObserveReport) -> Result<()> {
    ensure_parent(path)?;
    fs::write(path, serde_json::to_vec_pretty(report)?)
        .with_context(|| format!("failed to write report `{}`", path.display()))
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_xcode_destination_name() {
        assert_eq!(
            DestinationSelector::parse("platform=iOS Simulator,name=iPhone 16"),
            DestinationSelector {
                name: Some("iPhone 16".to_string()),
                udid: None,
                os: None
            }
        );
        assert_eq!(
            DestinationSelector::parse("platform=iOS Simulator,id=SIM-UDID,OS=18.0"),
            DestinationSelector {
                name: None,
                udid: Some("SIM-UDID".to_string()),
                os: Some("18.0".to_string())
            }
        );
        assert_eq!(
            DestinationSelector::parse("iPhone 16"),
            DestinationSelector {
                name: Some("iPhone 16".to_string()),
                udid: None,
                os: None
            }
        );
    }

    #[test]
    fn selects_available_simulator_from_simctl_json() {
        let selected = select_simulator(
            "platform=iOS Simulator,name=iPhone 16",
            &json!({
                "devices": {
                    "iOS 18.0": [
                        {"name": "iPhone 15", "udid": "old", "state": "Shutdown", "isAvailable": true},
                        {"name": "iPhone 16", "udid": "new", "state": "Booted", "isAvailable": true}
                    ]
                }
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(selected.name, "iPhone 16");
        assert_eq!(selected.udid, "new");
        assert_eq!(selected.state, "Booted");
    }

    #[test]
    fn selects_simulator_by_udid_and_honors_os_when_name_repeats() {
        let devices = json!({
            "devices": {
                "com.apple.CoreSimulator.SimRuntime.iOS-17-5": [
                    {"name": "iPhone 16", "udid": "ios-17", "state": "Shutdown", "isAvailable": true}
                ],
                "com.apple.CoreSimulator.SimRuntime.iOS-18-0": [
                    {"name": "iPhone 16", "udid": "ios-18", "state": "Booted", "isAvailable": true}
                ]
            }
        })
        .to_string();

        let by_udid = select_simulator("platform=iOS Simulator,id=ios-17", &devices).unwrap();
        assert_eq!(by_udid.udid, "ios-17");

        let by_os =
            select_simulator("platform=iOS Simulator,name=iPhone 16,OS=18.0", &devices).unwrap();
        assert_eq!(by_os.udid, "ios-18");
    }

    #[test]
    fn builds_xcode_and_simctl_command_shapes() {
        let options = ObserveOptions {
            scheme: Some("CutReadyCompanionApp".to_string()),
            destination: "platform=iOS Simulator,name=iPhone 16".to_string(),
            workspace: Some("CutReady.xcworkspace".into()),
            project: None,
            bundle_id: Some("com.example.cutready".to_string()),
            app_path: None,
            screenshot: None,
            report: "report.json".into(),
            diagnostics: None,
            log_predicate: None,
            log_seconds: 30,
            skip_build: false,
            skip_launch: false,
            json: true,
        };
        assert_eq!(
            build_command(&options),
            vec![
                "xcodebuild",
                "build",
                "-workspace",
                "CutReady.xcworkspace",
                "-scheme",
                "CutReadyCompanionApp",
                "-destination",
                "platform=iOS Simulator,name=iPhone 16"
            ]
        );
        assert_eq!(
            screenshot_command("SIM-UDID", Path::new("report/launch.png")),
            vec![
                "xcrun",
                "simctl",
                "io",
                "SIM-UDID",
                "screenshot",
                "report/launch.png"
            ]
        );
        assert_eq!(
            launch_command("SIM-UDID", Some("com.example.cutready")),
            vec![
                "xcrun",
                "simctl",
                "launch",
                "SIM-UDID",
                "com.example.cutready"
            ]
        );
    }

    #[test]
    fn summarizes_auditaur_apple_batches() {
        let summary = summarize_diagnostics(
            Path::new("diagnostics.json"),
            vec![json!({
                "schemaVersion": 1,
                "events": [{ "name": "cutready.auth.complete" }],
                "spans": [{ "name": "cutready.sync.push" }],
                "spanEvents": [{ "name": "rewrite.prompt.sent" }],
                "errors": [{ "name": "cutready.sync.push.error" }]
            })],
        );
        assert_eq!(summary.batch_count, 1);
        assert_eq!(summary.event_count, 1);
        assert_eq!(summary.span_count, 1);
        assert_eq!(summary.span_event_count, 1);
        assert_eq!(summary.error_count, 1);
    }

    #[test]
    fn observe_report_round_trips_as_agent_json() {
        let report = AppleObserveReport {
            schema_version: 1,
            generated_at: "2026-07-06T00:00:00Z".to_string(),
            destination: "platform=iOS Simulator,name=iPhone 16".to_string(),
            simulator: SimulatorSelection {
                name: "iPhone 16".to_string(),
                udid: "SIM-UDID".to_string(),
                state: "Booted".to_string(),
            },
            build: skipped_step("build", vec![]),
            install: skipped_step("install", vec![]),
            launch: skipped_step("launch", vec![]),
            screenshot: skipped_step("screenshot", vec![]),
            logs: summarize_logs("error: fixture\ninfo: ok\n", vec!["log".to_string()], None),
            diagnostics: summarize_diagnostics(
                Path::new("diagnostics.json"),
                vec![json!({
                    "events": [{ "name": "cutready.sketch.edit" }]
                })],
            ),
        };
        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["simulator"]["name"], "iPhone 16");
        assert_eq!(value["logs"]["errorCount"], 1);
        assert_eq!(value["diagnostics"]["eventCount"], 1);
        let round_trip: AppleObserveReport = serde_json::from_value(value).unwrap();
        assert_eq!(round_trip, report);
    }

    #[test]
    fn log_summary_counts_all_errors_but_keeps_recent_errors_bounded() {
        let logs = (0..12)
            .map(|index| format!("{{\"eventMessage\":\"error {index}\"}}"))
            .collect::<Vec<_>>()
            .join("\n");

        let summary = summarize_logs(&logs, vec!["log".to_string()], None);

        assert_eq!(summary.error_count, 12);
        assert_eq!(summary.recent_errors.len(), 10);
        assert!(summary.recent_errors[0].contains("error 2"));
        assert!(summary.recent_errors[9].contains("error 11"));
    }
}
