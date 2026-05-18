use anyhow::Result;
use auditaur_collector::exporter_sqlite::SqliteStore;
use auditaur_core::protocol::{DoctorCheck, DoctorReport};
use std::path::Path;

use crate::discovery::{self, DiscoveryStatus};

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
        None => match discovery::list_apps() {
            Ok(apps) if apps.is_empty() => checks.push(DoctorCheck {
                name: "discovery".to_string(),
                ok: true,
                message: "No discovery files found; pass --db to validate a specific database."
                    .to_string(),
            }),
            Ok(apps) => {
                let active = apps
                    .iter()
                    .filter(|app| {
                        app.status == DiscoveryStatus::Active
                            && app.database_readable
                            && app.schema_valid
                    })
                    .count();
                checks.push(DoctorCheck {
                    name: "discovery".to_string(),
                    ok: active > 0,
                    message: format!(
                        "Found {} discovery file(s), {active} active readable database(s).",
                        apps.len()
                    ),
                });
                for app in apps {
                    checks.push(DoctorCheck {
                        name: format!("discovery-db:{}", app.service_name),
                        ok: app.database_readable && app.schema_valid,
                        message: format!(
                            "{} database {} ({})",
                            if app.schema_valid { "Valid" } else { "Invalid" },
                            app.database_path,
                            app.stale_reason.unwrap_or_else(|| "active".to_string())
                        ),
                    });
                }
            }
            Err(error) => checks.push(DoctorCheck {
                name: "discovery".to_string(),
                ok: false,
                message: format!("Discovery check failed: {error}"),
            }),
        },
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
