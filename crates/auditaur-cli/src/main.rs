mod commands;
mod discovery;
pub mod mcp;
pub mod output;

use anyhow::Result;
use clap::{Parser, Subcommand};
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
            window,
            since,
            json,
            limit,
        } => commands::polish::related(&db, session, trace, window, since, json, limit),
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
