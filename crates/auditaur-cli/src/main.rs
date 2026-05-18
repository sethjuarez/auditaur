mod commands;
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
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Sessions {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Logs {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        trace: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },
    Errors {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        trace: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },
    Traces {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    Trace {
        trace_id: String,
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Mcp,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Doctor { db, json } => commands::doctor::run(db.as_deref(), json),
        Command::Sessions { db, json, limit } => commands::read::sessions(&db, json, limit),
        Command::Logs {
            db,
            session,
            trace,
            json,
            limit,
        } => commands::read::logs(&db, session, trace, json, limit),
        Command::Errors {
            db,
            session,
            trace,
            json,
            limit,
        } => commands::read::errors(&db, session, trace, json, limit),
        Command::Traces {
            db,
            session,
            json,
            limit,
        } => commands::read::traces(&db, session, json, limit),
        Command::Trace {
            trace_id,
            db,
            session,
            json,
        } => commands::read::trace(&db, session, trace_id, json),
        Command::Mcp => mcp::run(),
    }
}
