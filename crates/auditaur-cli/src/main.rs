mod commands;
mod discovery;
pub mod mcp;
pub mod output;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "auditaur",
    version,
    about = "Runtime observability for Tauri apps and AI agents.",
    after_help = "Bootstrap commands:\n  init skill [--path <repo-root>] [--agents-path] [--force] [--json]  Install the Auditaur debug agent skill"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Doctor {
        #[command(subcommand)]
        command: Option<DoctorCommand>,
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Apps {
        #[arg(long)]
        json: bool,
    },
    Health {
        #[arg(long)]
        json: bool,
    },
    Debug {
        #[command(flatten)]
        args: DebugArgs,
        #[command(subcommand)]
        command: Option<DebugCommand>,
    },
    Drive {
        #[command(flatten)]
        args: DriveArgs,
        #[command(subcommand)]
        command: Option<DriveCommand>,
    },
    Drill {
        #[command(subcommand)]
        command: DrillCommand,
    },
    Sessions {
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Logs {
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        trace: Option<String>,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },
    Errors {
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        trace: Option<String>,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },
    Exceptions {
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        trace: Option<String>,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        fingerprint: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        markdown: bool,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    Traces {
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        failed: bool,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    Trace {
        trace_id: String,
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        json: bool,
    },
    AgentRuns {
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        app: Option<String>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    AgentRun {
        run_id: String,
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        app: Option<String>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Ipc {
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        trace: Option<String>,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        failed: bool,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },
    Events {
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        trace: Option<String>,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },
    Windows {
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },
    Timeline {
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        trace: Option<String>,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },
    Related {
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        trace: Option<String>,
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long)]
        window: Option<String>,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },
    Explain {
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        trace: Option<String>,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },
    Bundle {
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        trace: Option<String>,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        redacted: bool,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long, default_value_t = 500)]
        limit: usize,
    },
    Tail {
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        trace: Option<String>,
        #[arg(long)]
        replay: bool,
        #[arg(long, default_value_t = 1000)]
        interval_ms: u64,
        #[arg(long)]
        duration_seconds: Option<u64>,
        #[arg(long)]
        json: bool,
    },
    Mcp,
}

#[derive(Debug, Subcommand)]
enum DoctorCommand {
    Tauri {
        #[arg(long)]
        path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Args)]
struct DebugArgs {
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    #[arg(long, global = true)]
    app: Option<String>,
    #[arg(long, global = true)]
    session_id: Option<String>,
    #[arg(long, global = true)]
    instance_id: Option<String>,
    #[arg(long, global = true)]
    pid: Option<u32>,
    #[arg(long, global = true)]
    latest: bool,
    #[arg(long, global = true)]
    active: bool,
    #[arg(long, global = true)]
    cdp_port: Option<u16>,
    #[arg(long, global = true)]
    require_frontend: bool,
    #[arg(long, global = true)]
    require_drive_bridge: bool,
    #[arg(long, global = true)]
    json: bool,
}

impl DebugArgs {
    fn selector(&self) -> commands::debug::DebugSelector {
        commands::debug::DebugSelector {
            db: self.db.clone(),
            app: self.app.clone(),
            session_id: self.session_id.clone(),
            instance_id: self.instance_id.clone(),
            pid: self.pid,
            latest: self.latest,
            active: self.active,
            cdp_port: self.cdp_port,
            require_frontend: self.require_frontend,
            require_drive_bridge: self.require_drive_bridge,
        }
    }
}

#[derive(Debug, Subcommand)]
enum DebugCommand {
    Status,
    Watch {
        #[arg(long, default_value_t = 500)]
        interval_ms: u64,
        #[arg(long)]
        timeout_seconds: Option<u64>,
        #[arg(long)]
        until_ready: bool,
    },
    Run {
        #[arg(long, default_value_t = 500)]
        interval_ms: u64,
        #[arg(long)]
        timeout_seconds: Option<u64>,
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
enum DrillCommand {
    Run {
        #[arg(long)]
        app: String,
        #[arg(long)]
        require_frontend: bool,
        #[arg(long)]
        require_drive_bridge: bool,
        #[arg(long, default_value_t = 180)]
        timeout_seconds: u64,
        #[arg(long, default_value_t = 500)]
        interval_ms: u64,
        #[arg(long, default_value = "auditaur-drill-report.json")]
        report: PathBuf,
        #[arg(long)]
        selector: Option<String>,
        #[arg(long)]
        expect_text: Option<String>,
        #[arg(long)]
        script: Option<PathBuf>,
        #[arg(long)]
        json: bool,
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
}

#[derive(Debug, Args)]
struct DriveArgs {
    #[arg(long, global = true)]
    app: Option<String>,
    #[arg(long, global = true)]
    session_id: Option<String>,
    #[arg(long, global = true)]
    instance_id: Option<String>,
    #[arg(long, global = true)]
    pid: Option<u32>,
    #[arg(long, global = true)]
    latest: bool,
    #[arg(long, global = true)]
    active: bool,
    #[arg(long, global = true, hide = true)]
    cdp_port: Option<u16>,
    #[arg(long, global = true)]
    json: bool,
}

impl DriveArgs {
    fn selector(&self) -> commands::drive::DriveAppSelector {
        commands::drive::DriveAppSelector {
            app: self.app.clone(),
            session_id: self.session_id.clone(),
            instance_id: self.instance_id.clone(),
            pid: self.pid,
            latest: self.latest,
            active: self.active,
        }
    }
}

#[derive(Debug, Subcommand)]
enum DriveCommand {
    Inspect,
    Wait {
        #[arg(long)]
        selector: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long, default_value_t = 5000)]
        timeout_ms: u64,
        #[arg(long)]
        test_id: Option<String>,
        #[arg(long)]
        step_id: Option<String>,
    },
    Exists {
        #[arg(long)]
        selector: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        test_id: Option<String>,
        #[arg(long)]
        step_id: Option<String>,
    },
    Text {
        #[arg(long)]
        selector: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        test_id: Option<String>,
        #[arg(long)]
        step_id: Option<String>,
    },
    Screenshot {
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        snapshot_output: Option<PathBuf>,
        #[arg(long)]
        selector: Option<String>,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        test_id: Option<String>,
        #[arg(long)]
        step_id: Option<String>,
    },
    Snapshot {
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        selector: Option<String>,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        test_id: Option<String>,
        #[arg(long)]
        step_id: Option<String>,
    },
    Click {
        #[arg(long)]
        selector: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long, hide = true)]
        allow_unproven_target: bool,
        #[arg(long, hide = true)]
        allow_probable_target: bool,
        #[arg(long)]
        test_id: Option<String>,
        #[arg(long)]
        step_id: Option<String>,
    },
    Fill {
        #[arg(long)]
        selector: String,
        #[arg(long)]
        value: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long, hide = true)]
        allow_unproven_target: bool,
        #[arg(long, hide = true)]
        allow_probable_target: bool,
        #[arg(long)]
        test_id: Option<String>,
        #[arg(long)]
        step_id: Option<String>,
    },
    Press {
        #[arg(long)]
        key: String,
        #[arg(long)]
        selector: Option<String>,
        #[arg(long)]
        target: Option<String>,
        #[arg(long, hide = true)]
        allow_unproven_target: bool,
        #[arg(long, hide = true)]
        allow_probable_target: bool,
        #[arg(long)]
        test_id: Option<String>,
        #[arg(long)]
        step_id: Option<String>,
    },
    Hover {
        #[arg(long)]
        selector: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long, hide = true)]
        allow_unproven_target: bool,
        #[arg(long, hide = true)]
        allow_probable_target: bool,
        #[arg(long)]
        test_id: Option<String>,
        #[arg(long)]
        step_id: Option<String>,
    },
    Select {
        #[arg(long)]
        selector: String,
        #[arg(long, required = true)]
        value: Vec<String>,
        #[arg(long)]
        target: Option<String>,
        #[arg(long, hide = true)]
        allow_unproven_target: bool,
        #[arg(long, hide = true)]
        allow_probable_target: bool,
        #[arg(long)]
        test_id: Option<String>,
        #[arg(long)]
        step_id: Option<String>,
    },
    Check {
        #[arg(long)]
        selector: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long, hide = true)]
        allow_unproven_target: bool,
        #[arg(long, hide = true)]
        allow_probable_target: bool,
        #[arg(long)]
        test_id: Option<String>,
        #[arg(long)]
        step_id: Option<String>,
    },
    Uncheck {
        #[arg(long)]
        selector: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long, hide = true)]
        allow_unproven_target: bool,
        #[arg(long, hide = true)]
        allow_probable_target: bool,
        #[arg(long)]
        test_id: Option<String>,
        #[arg(long)]
        step_id: Option<String>,
    },
    Evaluate {
        #[arg(long)]
        expression: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long, hide = true)]
        allow_unproven_target: bool,
        #[arg(long, hide = true)]
        allow_probable_target: bool,
        #[arg(long)]
        test_id: Option<String>,
        #[arg(long)]
        step_id: Option<String>,
    },
}

fn main() -> Result<()> {
    #[cfg(windows)]
    {
        return std::thread::Builder::new()
            .name("auditaur-main".to_string())
            .stack_size(8 * 1024 * 1024)
            .spawn(inner_main)?
            .join()
            .unwrap();
    }

    #[cfg(not(windows))]
    inner_main()
}

fn inner_main() -> Result<()> {
    let raw_args: Vec<String> = std::env::args().collect();
    if raw_args.get(1).is_some_and(|arg| arg == "init") {
        return commands::init::run(&raw_args[2..]);
    }
    if try_run_drive_type(&raw_args)? {
        return Ok(());
    }
    let (parse_args, drive_visible_only) = strip_drive_visible_args(raw_args);

    let cli = Cli::parse_from(parse_args);

    match cli.command {
        Command::Doctor { command, db, json } => match command {
            Some(DoctorCommand::Tauri { path, json }) => {
                commands::doctor::tauri(path.as_deref(), json)
            }

            None => commands::doctor::run(db.as_deref(), json),
        },
        Command::Apps { json } => commands::read::apps(json),
        Command::Health { json } => commands::health::run(json),
        Command::Debug { args, command } => match command.unwrap_or(DebugCommand::Status) {
            DebugCommand::Status => commands::debug::status(args.selector(), args.json),
            DebugCommand::Watch {
                interval_ms,
                timeout_seconds,
                until_ready,
            } => commands::debug::watch(
                args.selector(),
                interval_ms,
                timeout_seconds,
                until_ready,
                args.json,
            ),
            DebugCommand::Run {
                interval_ms,
                timeout_seconds,
                command,
            } => commands::debug::run(
                args.selector(),
                interval_ms,
                timeout_seconds,
                args.json,
                command,
            ),
        },
        Command::Drill { command } => match command {
            DrillCommand::Run {
                app,
                require_frontend,
                require_drive_bridge,
                timeout_seconds,
                interval_ms,
                report,
                selector,
                expect_text,
                script,
                json,
                command,
            } => {
                let exit_code = commands::drill::run(commands::drill::DrillRunOptions {
                    app,
                    require_frontend,
                    require_drive_bridge,
                    timeout_seconds,
                    interval_ms,
                    report,
                    selector,
                    expect_text,
                    script,
                    json,
                    command,
                })?;
                if exit_code != 0 {
                    std::process::exit(exit_code);
                }
                Ok(())
            }
        },
        Command::Drive { args, command } => match command {
            None => {
                let selector = args.selector();
                commands::drive::run(selector, args.cdp_port, args.json)
            }
            Some(DriveCommand::Inspect) => {
                let selector = args.selector();
                commands::drive::inspect(selector, args.cdp_port, args.json)
            }
            Some(DriveCommand::Wait {
                selector,
                target,
                timeout_ms,
                test_id,
                step_id,
            }) => {
                let app_selector = args.selector();
                commands::drive::wait(
                    app_selector,
                    args.cdp_port,
                    commands::drive::WaitOptions {
                        selector,
                        target_id: target,
                        timeout_ms,
                        test_id,
                        step_id,
                        visible_only: drive_visible_only,
                        json: args.json,
                    },
                )
            }
            Some(DriveCommand::Exists {
                selector,
                target,
                test_id,
                step_id,
            }) => {
                let app_selector = args.selector();
                commands::drive::exists(
                    app_selector,
                    args.cdp_port,
                    commands::drive::SelectorActionOptions {
                        selector,
                        target_id: target,
                        test_id,
                        step_id,
                        visible_only: drive_visible_only,
                        json: args.json,
                    },
                )
            }
            Some(DriveCommand::Text {
                selector,
                target,
                test_id,
                step_id,
            }) => {
                let app_selector = args.selector();
                commands::drive::text(
                    app_selector,
                    args.cdp_port,
                    commands::drive::SelectorActionOptions {
                        selector,
                        target_id: target,
                        test_id,
                        step_id,
                        visible_only: drive_visible_only,
                        json: args.json,
                    },
                )
            }
            Some(DriveCommand::Screenshot {
                output,
                snapshot_output,
                selector,
                target,
                test_id,
                step_id,
            }) => {
                let app_selector = args.selector();
                commands::drive::screenshot(
                    app_selector,
                    args.cdp_port,
                    commands::drive::ScreenshotOptions {
                        output,
                        snapshot_output,
                        selector,
                        target_id: target,
                        test_id,
                        step_id,
                        json: args.json,
                    },
                )
            }
            Some(DriveCommand::Snapshot {
                output,
                selector,
                target,
                test_id,
                step_id,
            }) => {
                let app_selector = args.selector();
                commands::drive::snapshot(
                    app_selector,
                    args.cdp_port,
                    commands::drive::SnapshotOptions {
                        output,
                        selector,
                        target_id: target,
                        test_id,
                        step_id,
                        json: args.json,
                    },
                )
            }
            Some(DriveCommand::Click {
                selector,
                target,
                allow_unproven_target: _,
                allow_probable_target: _,
                test_id,
                step_id,
            }) => {
                let app_selector = args.selector();
                commands::drive::click(
                    app_selector,
                    args.cdp_port,
                    commands::drive::SelectorActionOptions {
                        selector,
                        target_id: target,
                        test_id,
                        step_id,
                        visible_only: drive_visible_only,
                        json: args.json,
                    },
                )
            }
            Some(DriveCommand::Fill {
                selector,
                value,
                target,
                allow_unproven_target: _,
                allow_probable_target: _,
                test_id,
                step_id,
            }) => {
                let app_selector = args.selector();
                commands::drive::fill(
                    app_selector,
                    args.cdp_port,
                    commands::drive::FillOptions {
                        selector,
                        value,
                        target_id: target,
                        test_id,
                        step_id,
                        visible_only: drive_visible_only,
                        json: args.json,
                    },
                )
            }
            Some(DriveCommand::Press {
                key,
                selector,
                target,
                allow_unproven_target: _,
                allow_probable_target: _,
                test_id,
                step_id,
            }) => {
                let app_selector = args.selector();
                commands::drive::press(
                    app_selector,
                    args.cdp_port,
                    commands::drive::PressOptions {
                        key,
                        selector,
                        target_id: target,
                        test_id,
                        step_id,
                        json: args.json,
                    },
                )
            }
            Some(DriveCommand::Hover {
                selector,
                target,
                allow_unproven_target: _,
                allow_probable_target: _,
                test_id,
                step_id,
            }) => {
                let app_selector = args.selector();
                commands::drive::hover(
                    app_selector,
                    args.cdp_port,
                    commands::drive::SelectorActionOptions {
                        selector,
                        target_id: target,
                        test_id,
                        step_id,
                        visible_only: drive_visible_only,
                        json: args.json,
                    },
                )
            }
            Some(DriveCommand::Select {
                selector,
                value,
                target,
                allow_unproven_target: _,
                allow_probable_target: _,
                test_id,
                step_id,
            }) => {
                let app_selector = args.selector();
                commands::drive::select(
                    app_selector,
                    args.cdp_port,
                    commands::drive::SelectOptions {
                        selector,
                        values: value,
                        target_id: target,
                        test_id,
                        step_id,
                        visible_only: drive_visible_only,
                        json: args.json,
                    },
                )
            }
            Some(DriveCommand::Check {
                selector,
                target,
                allow_unproven_target: _,
                allow_probable_target: _,
                test_id,
                step_id,
            }) => {
                let app_selector = args.selector();
                commands::drive::check(
                    app_selector,
                    args.cdp_port,
                    commands::drive::SelectorActionOptions {
                        selector,
                        target_id: target,
                        test_id,
                        step_id,
                        visible_only: drive_visible_only,
                        json: args.json,
                    },
                )
            }
            Some(DriveCommand::Uncheck {
                selector,
                target,
                allow_unproven_target: _,
                allow_probable_target: _,
                test_id,
                step_id,
            }) => {
                let app_selector = args.selector();
                commands::drive::uncheck(
                    app_selector,
                    args.cdp_port,
                    commands::drive::SelectorActionOptions {
                        selector,
                        target_id: target,
                        test_id,
                        step_id,
                        visible_only: drive_visible_only,
                        json: args.json,
                    },
                )
            }
            Some(DriveCommand::Evaluate {
                expression,
                target,
                allow_unproven_target: _,
                allow_probable_target: _,
                test_id,
                step_id,
            }) => {
                let app_selector = args.selector();
                commands::drive::evaluate(
                    app_selector,
                    args.cdp_port,
                    commands::drive::EvaluateOptions {
                        expression,
                        target_id: target,
                        test_id,
                        step_id,
                        json: args.json,
                    },
                )
            }
        },
        Command::Sessions { db, json, limit } => commands::read::sessions(&db, json, limit),
        Command::Logs {
            db,
            session,
            trace,
            since,
            json,
            limit,
        } => commands::read::logs(&db, session, trace, since, json, limit),
        Command::Errors {
            db,
            session,
            trace,
            since,
            json,
            limit,
        } => commands::read::errors(&db, session, trace, since, json, limit),
        Command::Exceptions {
            db,
            session,
            trace,
            since,
            fingerprint,
            json,
            markdown,
            output,
            limit,
        } => commands::exceptions::run(
            &db,
            commands::exceptions::ExceptionOptions {
                session_id: session,
                trace_id: trace,
                since,
                fingerprint,
                json,
                markdown,
                output,
                limit,
            },
        ),
        Command::Traces {
            db,
            session,
            since,
            failed,
            json,
            limit,
        } => commands::read::traces(&db, session, since, failed, json, limit),
        Command::Trace {
            trace_id,
            db,
            session,
            json,
        } => commands::read::trace(&db, session, trace_id, json),
        Command::AgentRuns {
            db,
            app,
            session,
            since,
            json,
            limit,
        } => commands::agent::runs(&db, app, session, since, json, limit),
        Command::AgentRun {
            run_id,
            db,
            app,
            session,
            json,
        } => commands::agent::run(&db, app, session, run_id, json),
        Command::Ipc {
            db,
            session,
            trace,
            since,
            failed,
            json,
            limit,
        } => commands::read::ipc(&db, session, trace, since, failed, json, limit),
        Command::Events {
            db,
            session,
            trace,
            since,
            json,
            limit,
        } => commands::read::events(&db, session, trace, since, json, limit),
        Command::Windows {
            db,
            session,
            json,
            limit,
        } => commands::read::windows(&db, session, json, limit),
        Command::Timeline {
            db,
            session,
            trace,
            since,
            json,
            limit,
        } => commands::polish::timeline(&db, session, trace, since, json, limit),
        Command::Related {
            db,
            session,
            trace,
            run_id,
            window,
            since,
            json,
            limit,
        } => commands::polish::related(&db, session, trace, run_id, window, since, json, limit),
        Command::Explain {
            db,
            session,
            trace,
            since,
            json,
            limit,
        } => commands::polish::explain(&db, session, trace, since, json, limit),
        Command::Bundle {
            db,
            session,
            trace,
            since,
            redacted,
            output,
            limit,
        } => commands::polish::bundle(&db, session, trace, since, redacted, output, limit),
        Command::Tail {
            db,
            session,
            trace,
            replay,
            interval_ms,
            duration_seconds,
            json,
        } => commands::polish::tail(
            &db,
            session,
            trace,
            replay,
            interval_ms,
            duration_seconds,
            json,
        ),
        Command::Mcp => mcp::run(),
    }
}

fn try_run_drive_type(raw_args: &[String]) -> Result<bool> {
    let Some(drive_index) = raw_args.iter().position(|arg| arg == "drive") else {
        return Ok(false);
    };
    let Some((type_index, "type")) = find_drive_subcommand(raw_args, drive_index)? else {
        return Ok(false);
    };

    let mut app_selector = commands::drive::DriveAppSelector {
        app: None,
        session_id: None,
        instance_id: None,
        pid: None,
        latest: false,
        active: false,
    };
    let mut cdp_port = None;
    let mut options = commands::drive::TypeOptions {
        selector: String::new(),
        value: String::new(),
        target_id: None,
        test_id: None,
        step_id: None,
        visible_only: false,
        json: false,
    };

    let mut index = drive_index + 1;
    while index < raw_args.len() {
        if index == type_index {
            index += 1;
            continue;
        }
        match raw_args[index].as_str() {
            "--app" => app_selector.app = Some(next_arg(raw_args, &mut index, "--app")?),
            "--session-id" => {
                app_selector.session_id = Some(next_arg(raw_args, &mut index, "--session-id")?)
            }
            "--instance-id" => {
                app_selector.instance_id = Some(next_arg(raw_args, &mut index, "--instance-id")?)
            }
            "--pid" => {
                app_selector.pid = Some(
                    next_arg(raw_args, &mut index, "--pid")?
                        .parse()
                        .map_err(|_| anyhow::anyhow!("Invalid --pid value."))?,
                )
            }
            "--latest" => app_selector.latest = true,
            "--active" => app_selector.active = true,
            "--cdp-port" => {
                cdp_port = Some(
                    next_arg(raw_args, &mut index, "--cdp-port")?
                        .parse()
                        .map_err(|_| anyhow::anyhow!("Invalid --cdp-port value."))?,
                )
            }
            "--json" => options.json = true,
            "--selector" => options.selector = next_arg(raw_args, &mut index, "--selector")?,
            "--value" => options.value = next_arg(raw_args, &mut index, "--value")?,
            "--target" => options.target_id = Some(next_arg(raw_args, &mut index, "--target")?),
            "--test-id" => options.test_id = Some(next_arg(raw_args, &mut index, "--test-id")?),
            "--step-id" => options.step_id = Some(next_arg(raw_args, &mut index, "--step-id")?),
            "--allow-unproven-target" | "--allow-probable-target" => {}
            "--visible" | "--visible-only" => options.visible_only = true,
            unknown => {
                return Err(anyhow::anyhow!(
                    "Unknown `auditaur drive type` argument `{unknown}`."
                ))
            }
        }
        index += 1;
    }

    if options.selector.is_empty() {
        return Err(anyhow::anyhow!(
            "`auditaur drive type` requires --selector <css>."
        ));
    }
    commands::drive::type_text(app_selector, cdp_port, options)?;
    Ok(true)
}

fn find_drive_subcommand<'a>(
    raw_args: &'a [String],
    drive_index: usize,
) -> Result<Option<(usize, &'a str)>> {
    let mut index = drive_index + 1;
    while index < raw_args.len() {
        match raw_args[index].as_str() {
            "--app" | "--session-id" | "--instance-id" | "--pid" | "--cdp-port" => {
                let flag = raw_args[index].clone();
                let _ = next_arg(raw_args, &mut index, &flag)?;
            }
            "--latest" | "--active" | "--json" => {}
            flag if flag.starts_with("--") => return Ok(None),
            command => return Ok(Some((index, command))),
        }
        index += 1;
    }
    Ok(None)
}

fn strip_drive_visible_args(raw_args: Vec<String>) -> (Vec<String>, bool) {
    if !raw_args.iter().any(|arg| arg == "drive") {
        return (raw_args, false);
    }
    let mut visible_only = false;
    let args = raw_args
        .into_iter()
        .filter(|arg| {
            if arg == "--visible" || arg == "--visible-only" {
                visible_only = true;
                false
            } else {
                true
            }
        })
        .collect();
    (args, visible_only)
}

fn next_arg(raw_args: &[String], index: &mut usize, flag: &str) -> Result<String> {
    *index += 1;
    raw_args
        .get(*index)
        .filter(|value| !value.starts_with("--"))
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("`{flag}` requires a value."))
}
