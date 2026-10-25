//! MCP tools for drill tests. Reads that depend on the working directory shell out to
//! `auditaur drill ... --json` in the requested `cwd` so output matches the CLI exactly;
//! `drill_test` starts a background run and returns its id for `drill_show` to poll.

use std::{
    collections::HashSet,
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

use crate::commands::drills::{self, run};

/// How long `drill_test` waits for the run folder before returning without a run id.
const START_TIMEOUT: Duration = Duration::from_secs(20);
const LOG_TAIL_BYTES: usize = 4_000;
const UPDATE_MODES: &[&str] = &["none", "missing", "changed", "all"];
/// The background run's stdout/stderr, moved into its run folder when it exits.
const RUN_LOG_FILE: &str = "drill-test.log";

pub fn call(name: &str, arguments: &Value) -> Option<Result<Value>> {
    let schema = tools().into_iter().find(|tool| tool["name"] == name)?;
    if let Some(unknown) = arguments.as_object().and_then(|arguments| {
        arguments.keys().find(|key| {
            schema["inputSchema"]["properties"]
                .get(key.as_str())
                .is_none()
        })
    }) {
        return Some(Err(anyhow!("{name} does not take `{unknown}`")));
    }
    let result = match name {
        "drill_list" => list(arguments),
        "drill_validate" => validate(arguments),
        "drill_test" => test(arguments),
        "drill_runs" => workspace(arguments).and_then(|cwd| drills::runs_json(&cwd)),
        "drill_show" => show(arguments),
        "drill_cancel" => cancel(arguments),
        _ => return None,
    };
    Some(result)
}

fn list(arguments: &Value) -> Result<Value> {
    let mut args = vec![
        "drill".to_string(),
        "list".to_string(),
        "--json".to_string(),
    ];
    push_filters(&mut args, arguments)?;
    cli_json(&workspace(arguments)?, &args)
}

fn validate(arguments: &Value) -> Result<Value> {
    let mut args = vec![
        "drill".to_string(),
        "validate".to_string(),
        "--json".to_string(),
    ];
    for path in string_array(arguments, "paths")? {
        args.push(positional(path, "paths")?);
    }
    cli_json(&workspace(arguments)?, &args)
}

fn show(arguments: &Value) -> Result<Value> {
    let cwd = workspace(arguments)?;
    let selector = string(arguments, "runId")?;
    match string(arguments, "test")? {
        Some(test) => drills::test_detail_json(&cwd, selector.as_deref(), &test),
        None => drills::show_json(&cwd, selector.as_deref()),
    }
}

fn cancel(arguments: &Value) -> Result<Value> {
    let cwd = workspace(arguments)?;
    let selector = string(arguments, "runId")?;
    let dir = run::resolve_run(&cwd.join(run::RUNS_DIR), selector.as_deref())?;
    let report = run::read_run(&dir)?;
    if report.status != run::Status::Running {
        bail!(
            "run {} is not running (status: {})",
            report.run_id,
            report.status.as_str()
        );
    }
    fs::write(dir.join(run::CANCEL_REQUEST_FILE), b"")
        .with_context(|| format!("failed to request cancel for run {}", report.run_id))?;
    Ok(json!({
        "runId": report.run_id,
        "cancelRequested": true,
        "note": "The run finishes its current step, runs teardown, and ends as interrupted; poll drill_show.",
    }))
}

fn test(arguments: &Value) -> Result<Value> {
    let cwd = workspace(arguments)?;
    let mut args = vec!["drill".to_string(), "test".to_string()];
    push_filters(&mut args, arguments)?;
    if let Some(app) = string(arguments, "app")? {
        args.push(format!("--app={app}"));
    }
    for (key, flag) in [
        ("attach", "--attach"),
        ("requireFrontend", "--require-frontend"),
        ("lastFailed", "--last-failed"),
    ] {
        if boolean(arguments, key)? {
            args.push(flag.to_string());
        }
    }
    for (key, flag) in [
        ("retries", "--retries"),
        ("maxFailures", "--max-failures"),
        ("timeoutSeconds", "--timeout-seconds"),
    ] {
        if let Some(value) = unsigned(arguments, key)? {
            args.push(format!("{flag}={value}"));
        }
    }
    if let Some(mode) = string(arguments, "updateSnapshots")? {
        if !UPDATE_MODES.contains(&mode.as_str()) {
            bail!(
                "`updateSnapshots` must be one of {}",
                UPDATE_MODES.join(", ")
            );
        }
        args.push(format!("--update-snapshots={mode}"));
    }
    let command = string_array(arguments, "command")?;
    if !command.is_empty() {
        args.push("--".to_string());
        args.extend(command);
    }
    start_run(&cwd, &args)
}

/// Spawn `auditaur drill test` in the background and wait for its run folder.
fn start_run(cwd: &Path, args: &[String]) -> Result<Value> {
    let runs_root = cwd.join(run::RUNS_DIR);
    // Only a run created after the spawn can be ours, even if an old run shares the pid.
    let existing: HashSet<String> = run::run_ids(&runs_root)
        .unwrap_or_default()
        .into_iter()
        .collect();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    let log = std::env::temp_dir().join(format!(
        "auditaur-drill-test-{}-{stamp}.log",
        std::process::id()
    ));
    let output =
        File::create(&log).with_context(|| format!("failed to create `{}`", log.display()))?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        // Never inherit stdout: it carries this server's JSON-RPC stream.
        .stdout(output.try_clone()?)
        .stderr(output);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().context("failed to start `drill test`")?;
    let pid = child.id();
    let started = Instant::now();
    loop {
        if let Some(report) = new_run_owned_by(&runs_root, &existing, pid) {
            supervise(child, log.clone(), runs_root.join(&report.run_id));
            return Ok(json!({
                "started": true,
                "runId": report.run_id,
                "pid": pid,
                "status": report.status,
                "tests": report.totals.tests,
                "dir": format!("{}/{}", run::RUNS_DIR, report.run_id),
                "log": log.display().to_string(),
                "next": "Poll drill_show with this runId until status is no longer `running`; use drill_show with `test` for failure details. The log moves into the run folder as drill-test.log when the run ends.",
            }));
        }
        if let Some(status) = child.try_wait()? {
            // The run may have finished between the folder check and the exit check.
            if let Some(report) = new_run_owned_by(&runs_root, &existing, pid) {
                let dir = runs_root.join(&report.run_id);
                let log = keep_log(&log, &dir);
                return Ok(json!({
                    "started": true,
                    "runId": report.run_id,
                    "pid": pid,
                    "status": report.status,
                    "exitCode": status.code(),
                    "totals": report.totals,
                    "dir": format!("{}/{}", run::RUNS_DIR, report.run_id),
                    "log": log.display().to_string(),
                }));
            }
            let tail = log_tail(&log);
            let _ = fs::remove_file(&log);
            return match status.code() {
                Some(0) => Ok(json!({ "started": false, "exitCode": 0, "output": tail })),
                code => Err(anyhow!(
                    "`drill test` exited with code {} before starting a run:\n{tail}",
                    code.map_or_else(|| "unknown".to_string(), |code| code.to_string())
                )),
            };
        }
        if started.elapsed() > START_TIMEOUT {
            let (root, existing, log_file) = (runs_root.clone(), existing.clone(), log.clone());
            thread::spawn(move || {
                let _ = child.wait();
                // Without a run folder the temp log is the only record, so leave it.
                if let Some(report) = new_run_owned_by(&root, &existing, pid) {
                    keep_log(&log_file, &root.join(report.run_id));
                }
            });
            return Ok(json!({
                "started": true,
                "runId": null,
                "pid": pid,
                "log": log.display().to_string(),
                "next": "The run folder has not appeared yet; check drill_runs shortly or read the log.",
            }));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// The run created since `existing` whose `run.json` names `pid` as its owner.
fn new_run_owned_by(root: &Path, existing: &HashSet<String>, pid: u32) -> Option<run::RunReport> {
    let ids = run::run_ids(root).ok()?;
    ids.iter()
        .rev()
        .filter(|id| !existing.contains(*id))
        .filter_map(|id| run::read_run(&root.join(id)).ok())
        .find(|report| report.pid == pid)
}

/// Reap the child when it exits, then move its log into the run folder so run pruning owns it.
fn supervise(mut child: Child, log: PathBuf, dir: PathBuf) {
    thread::spawn(move || {
        let _ = child.wait();
        keep_log(&log, &dir);
    });
}

/// Move a temp log into `dir` as `drill-test.log`; returns where the log now lives.
fn keep_log(log: &Path, dir: &Path) -> PathBuf {
    let target = dir.join(RUN_LOG_FILE);
    if fs::copy(log, &target).is_ok() {
        let _ = fs::remove_file(log);
        target
    } else {
        log.to_path_buf()
    }
}

fn log_tail(path: &Path) -> String {
    let text = fs::read(path).unwrap_or_default();
    let start = text.len().saturating_sub(LOG_TAIL_BYTES);
    String::from_utf8_lossy(&text[start..]).trim().to_string()
}

/// Run `auditaur <args>` in `cwd` and parse its JSON stdout, even on a non-zero exit
/// (e.g. `validate` reports invalid files as JSON and exits 1).
fn cli_json(cwd: &Path, args: &[String]) -> Result<Value> {
    let output = Command::new(std::env::current_exe()?)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()
        .context("failed to run auditaur")?;
    serde_json::from_slice(&output.stdout).map_err(|_| {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow!("`auditaur {}` failed: {}", args.join(" "), stderr.trim())
    })
}

fn push_filters(args: &mut Vec<String>, arguments: &Value) -> Result<()> {
    for filter in string_array(arguments, "filters")? {
        args.push(positional(filter, "filters")?);
    }
    if let Some(grep) = string(arguments, "grep")? {
        args.push(format!("--grep={grep}"));
    }
    if let Some(grep) = string(arguments, "grepInvert")? {
        args.push(format!("--grep-invert={grep}"));
    }
    for tag in string_array(arguments, "tags")? {
        args.push(format!("--tag={tag}"));
    }
    Ok(())
}

/// Positional values must not look like flags.
fn positional(value: String, key: &str) -> Result<String> {
    if value.starts_with('-') {
        bail!("`{key}` entries cannot start with `-`: {value:?}");
    }
    Ok(value)
}

/// `cwd` argument, else the server's working directory.
fn workspace(arguments: &Value) -> Result<PathBuf> {
    let cwd = match string(arguments, "cwd")? {
        Some(cwd) => PathBuf::from(cwd),
        None => return Ok(std::env::current_dir()?),
    };
    if !cwd.is_dir() {
        bail!("`cwd` `{}` is not a directory", cwd.display());
    }
    Ok(cwd)
}

fn string(arguments: &Value, key: &str) -> Result<Option<String>> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(anyhow!("`{key}` must be a string")),
    }
}

fn boolean(arguments: &Value, key: &str) -> Result<bool> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(anyhow!("`{key}` must be a boolean")),
    }
}

fn unsigned(arguments: &Value, key: &str) -> Result<Option<u64>> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| anyhow!("`{key}` must be a non-negative integer")),
    }
}

fn string_array(arguments: &Value, key: &str) -> Result<Vec<String>> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| anyhow!("`{key}` must be an array of strings"))
            })
            .collect(),
        Some(_) => Err(anyhow!("`{key}` must be an array of strings")),
    }
}

pub fn tools() -> Vec<Value> {
    let cwd = json!({ "type": "string", "description": "Workspace folder containing .auditaur/drills; defaults to the server's working directory." });
    let filters = json!({
        "filters": { "type": "array", "items": { "type": "string" }, "description": "File path substrings or suite names." },
        "grep": { "type": "string", "description": "Regex matched against `suite > test` and @tags." },
        "grepInvert": { "type": "string", "description": "Regex of tests to skip." },
        "tags": { "type": "array", "items": { "type": "string" } },
    });
    let with = |extra: Value| {
        let mut properties = json!({ "cwd": cwd });
        for source in [&filters, &extra] {
            for (key, value) in source.as_object().into_iter().flatten() {
                properties[key] = value.clone();
            }
        }
        properties
    };
    let run_id = json!({ "type": "string", "description": "Run id or unique prefix; defaults to the latest run." });
    vec![
        drill_tool(
            "drill_list",
            "List drill test suites and tests (ids, titles, tags, steps) from .auditaur/drills, with optional filters.",
            with(json!({})),
        ),
        drill_tool(
            "drill_validate",
            "Validate drill files against the schema, with \"did you mean\" hints. Validates .auditaur/drills unless paths are given.",
            json!({ "cwd": cwd, "paths": { "type": "array", "items": { "type": "string" } } }),
        ),
        drill_tool(
            "drill_test",
            "Start `auditaur drill test` in the background and return its runId. Poll drill_show until status is not `running`. Launches the app from `command`, the config `start` command, or `attach` to a running app.",
            with(json!({
                "app": { "type": "string" },
                "attach": { "type": "boolean" },
                "command": { "type": "array", "items": { "type": "string" }, "description": "Launch command, e.g. [\"npm\", \"run\", \"tauri\", \"dev\"]." },
                "requireFrontend": { "type": "boolean" },
                "timeoutSeconds": { "type": "integer", "minimum": 0, "description": "App readiness timeout." },
                "retries": { "type": "integer", "minimum": 0 },
                "maxFailures": { "type": "integer", "minimum": 1 },
                "lastFailed": { "type": "boolean" },
                "updateSnapshots": { "type": "string", "enum": UPDATE_MODES },
            })),
        ),
        drill_tool(
            "drill_runs",
            "List recorded drill runs, newest first, with status and totals.",
            json!({ "cwd": cwd }),
        ),
        drill_tool(
            "drill_show",
            "Show a drill run report (suites, tests, outcomes, first errors). Pass `test` (id, `suite > test` title, or name) for that test's attempts with every step, error, and attachment.",
            json!({ "cwd": cwd, "runId": run_id, "test": { "type": "string" } }),
        ),
        drill_tool(
            "drill_cancel",
            "Ask a running drill run to stop: it finishes the current step, runs teardown, and ends as interrupted.",
            json!({ "cwd": cwd, "runId": run_id }),
        ),
    ]
}

fn drill_tool(name: &str, description: &str, properties: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "additionalProperties": false
        }
    })
}
