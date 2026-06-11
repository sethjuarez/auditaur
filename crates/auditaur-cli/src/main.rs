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
    about = "Runtime observability for Tauri apps and AI agents."
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
    Drive {
        #[command(flatten)]
        args: DriveArgs,
        #[command(subcommand)]
        command: Option<DriveCommand>,
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
    #[arg(long, global = true)]
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
        #[arg(long)]
        test_id: Option<String>,
        #[arg(long)]
        step_id: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Doctor { command, db, json } => match command {
            Some(DoctorCommand::Tauri { path, json }) => {
                commands::doctor::tauri(path.as_deref(), json)
            }
            None => commands::doctor::run(db.as_deref(), json),
        },
        Command::Apps { json } => commands::read::apps(json),
        Command::Health { json } => commands::health::run(json),
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
                        json: args.json,
                    },
                )
            }
            Some(DriveCommand::Screenshot {
                output,
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
                        json: args.json,
                    },
                )
            }
            Some(DriveCommand::Fill {
                selector,
                value,
                target,
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
                        json: args.json,
                    },
                )
            }
            Some(DriveCommand::Press {
                key,
                selector,
                target,
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
