use anyhow::Result;
use auditaur_collector::exporter_sqlite::SqliteStore;
use auditaur_core::protocol::{DoctorCheck, DoctorReport};
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::discovery::{self, DiscoveryStatus};

pub fn run(db: Option<&Path>, json: bool) -> Result<()> {
    let report = report(db);
    print_report("Auditaur doctor", report, json)
}

pub fn tauri(path: Option<&Path>, json: bool) -> Result<()> {
    let root = path.map(PathBuf::from).unwrap_or(std::env::current_dir()?);
    let report = tauri_report(&root);
    print_report("Auditaur Tauri doctor", report, json)
}

pub fn report(db: Option<&Path>) -> DoctorReport {
    let mut checks = Vec::new();

    match db {
        Some(path) if path.exists() => {
            match SqliteStore::open(path).and_then(|store| {
                store.migrate()?;
                store.validate_schema()
            }) {
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

    DoctorReport {
        ok: checks.iter().all(|check| check.ok),
        checks,
    }
}

fn tauri_report(root: &Path) -> DoctorReport {
    let app_dir = if root.join("src-tauri").is_dir() {
        root.join("src-tauri")
    } else {
        root.to_path_buf()
    };
    let mut checks = Vec::new();
    let cargo_toml = app_dir.join("Cargo.toml");
    let cargo_text = fs::read_to_string(&cargo_toml);
    checks.push(match &cargo_text {
        Ok(text) if text.contains("tauri-plugin-auditaur") => DoctorCheck {
            name: "tauri-plugin-dependency".to_string(),
            ok: true,
            message: format!("Found tauri-plugin-auditaur in {}", cargo_toml.display()),
        },
        Ok(_) => DoctorCheck {
            name: "tauri-plugin-dependency".to_string(),
            ok: false,
            message: format!("Missing tauri-plugin-auditaur in {}", cargo_toml.display()),
        },
        Err(error) => DoctorCheck {
            name: "tauri-plugin-dependency".to_string(),
            ok: false,
            message: format!("Could not read {}: {error}", cargo_toml.display()),
        },
    });

    let capabilities_dir = app_dir.join("capabilities");
    let capability_text = read_text_files(&capabilities_dir);
    checks.push(if capability_text.contains("auditaur:default") {
        DoctorCheck {
            name: "auditaur-permission".to_string(),
            ok: true,
            message: format!("Found auditaur:default under {}", capabilities_dir.display()),
        }
    } else {
        DoctorCheck {
            name: "auditaur-permission".to_string(),
            ok: false,
            message: format!(
                "Missing auditaur:default under {}; frontend export_otel_batch calls will be blocked.",
                capabilities_dir.display()
            ),
        }
    });

    let source_text = read_text_files(&app_dir.join("src"));
    checks.push(
        if source_text.contains("tauri_plugin_auditaur::Builder")
            || source_text.contains("tauri_plugin_auditaur::init")
            || source_text.contains("tauri-plugin-auditaur")
        {
            DoctorCheck {
                name: "plugin-registration".to_string(),
                ok: true,
                message: "Found Auditaur plugin registration reference in src.".to_string(),
            }
        } else {
            DoctorCheck {
                name: "plugin-registration".to_string(),
                ok: false,
                message: "No Auditaur plugin registration reference found in src.".to_string(),
            }
        },
    );

    checks.push(if source_text.contains("tracing_layer()") {
        DoctorCheck {
            name: "tracing-layer".to_string(),
            ok: true,
            message: "Found tauri_plugin_auditaur::tracing_layer usage.".to_string(),
        }
    } else {
        DoctorCheck {
            name: "tracing-layer".to_string(),
            ok: true,
            message: "No Auditaur tracing layer found; backend tracing capture is optional."
                .to_string(),
        }
    });

    DoctorReport {
        ok: checks.iter().all(|check| check.ok),
        checks,
    }
}

fn print_report(title: &str, report: DoctorReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("{}: {}", title, if report.ok { "ok" } else { "failed" });
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

fn read_text_files(path: &Path) -> String {
    let mut text = String::new();
    read_text_files_into(path, &mut text);
    text
}

fn read_text_files_into(path: &Path, output: &mut String) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    if metadata.is_file() {
        if is_text_candidate(path) {
            if let Ok(text) = fs::read_to_string(path) {
                output.push_str(&text);
                output.push('\n');
            }
        }
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        read_text_files_into(&entry.path(), output);
    }
}

fn is_text_candidate(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("rs" | "toml" | "json" | "json5")
    )
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
