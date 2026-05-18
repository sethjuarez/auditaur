use anyhow::Result;
use auditaur_collector::exporter_sqlite::SqliteStore;
use auditaur_core::protocol::{DoctorCheck, DoctorReport};
use std::path::Path;

pub fn run(db: Option<&Path>, json: bool) -> Result<()> {
    let report = report(db);

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

pub fn report(db: Option<&Path>) -> DoctorReport {
    let mut checks = Vec::new();

    match db {
        Some(path) if path.exists() => {
            match SqliteStore::open(path).and_then(|store| store.validate_schema()) {
                Ok(()) => checks.push(DoctorCheck {
                    name: "sqlite-schema".to_string(),
                    ok: true,
                    message: format!("Database schema is valid: {}", path.display()),
                }),
                Err(error) => checks.push(DoctorCheck {
                    name: "sqlite-schema".to_string(),
                    ok: false,
                    message: format!("Database schema is invalid: {error}"),
                }),
            }
        }
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

    report
}

#[cfg(test)]
mod tests {
    use super::report;
    use auditaur_collector::exporter_sqlite::SqliteStore;
    use tempfile::NamedTempFile;

    #[test]
    fn validates_sqlite_schema() {
        let db = NamedTempFile::new().unwrap();
        let store = SqliteStore::open(db.path()).unwrap();
        store.migrate().unwrap();
        drop(store);

        let report = report(Some(db.path()));

        assert!(report.ok);
        assert_eq!(report.checks[0].name, "sqlite-schema");
    }
}
