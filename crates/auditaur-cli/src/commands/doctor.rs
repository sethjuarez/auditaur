use anyhow::Result;
use auditaur_core::protocol::{DoctorCheck, DoctorReport};
use std::path::Path;

pub fn run(db: Option<&Path>, json: bool) -> Result<()> {
    let mut checks = Vec::new();

    match db {
        Some(path) if path.exists() => checks.push(DoctorCheck {
            name: "database-path".to_string(),
            ok: true,
            message: format!("Database path exists: {}", path.display()),
        }),
        Some(path) => checks.push(DoctorCheck {
            name: "database-path".to_string(),
            ok: false,
            message: format!("Database path does not exist: {}", path.display()),
        }),
        None => checks.push(DoctorCheck {
            name: "database-path".to_string(),
            ok: true,
            message: "No database path provided; discovery checks are not implemented yet."
                .to_string(),
        }),
    }

    let report = DoctorReport {
        ok: checks.iter().all(|check| check.ok),
        checks,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "Auditaur doctor: {}",
            if report.ok { "ok" } else { "failed" }
        );
        for check in report.checks {
            println!(
                "{} {} - {}",
                if check.ok { "ok" } else { "fail" },
                check.name,
                check.message
            );
        }
    }

    Ok(())
}
