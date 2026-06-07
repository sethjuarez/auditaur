use anyhow::Result;
use auditaur_core::protocol::{AppHealth, HealthCheck, HealthReport};

use crate::{
    discovery::{self, DiscoveryStatus},
    output::table_cell,
};

const EXPECTED_CAPABILITIES: &[&str] = &[
    "events",
    "frontend_errors",
    "ipc",
    "logs",
    "traces",
    "windows",
];

pub fn run(json: bool) -> Result<()> {
    let report = report();
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_report(&report);
    }
    Ok(())
}

pub fn report() -> HealthReport {
    match discovery::list_apps() {
        Ok(apps) => {
            let app_health: Vec<_> = apps.into_iter().map(app_health).collect();
            let active_count = app_health
                .iter()
                .filter(|app| app.status == "active")
                .count();
            let unhealthy_active = app_health
                .iter()
                .filter(|app| app.status == "active" && !app.ok)
                .count();
            let ok = unhealthy_active == 0;
            let summary = match (active_count, unhealthy_active, app_health.len()) {
                (0, _, 0) => "No Auditaur apps discovered.".to_string(),
                (0, _, total) => {
                    format!("No active Auditaur apps; {total} stale app(s) discovered.")
                }
                (_, 0, _) => format!("{active_count} active Auditaur app(s) healthy."),
                _ => format!(
                    "{unhealthy_active} of {active_count} active Auditaur app(s) have health issues."
                ),
            };
            HealthReport {
                ok,
                summary,
                apps: app_health,
            }
        }
        Err(error) => HealthReport {
            ok: false,
            summary: format!("Discovery failed: {error}"),
            apps: Vec::new(),
        },
    }
}

fn app_health(app: discovery::DiscoveredApp) -> AppHealth {
    let mut checks = Vec::new();
    checks.push(HealthCheck {
        name: "heartbeat".to_string(),
        ok: app.status == DiscoveryStatus::Active,
        message: app
            .stale_reason
            .clone()
            .unwrap_or_else(|| "heartbeat is fresh".to_string()),
    });
    checks.push(HealthCheck {
        name: "database-readable".to_string(),
        ok: app.database_readable,
        message: if app.database_readable {
            format!("database is readable: {}", app.database_path)
        } else {
            format!("database is not readable: {}", app.database_path)
        },
    });
    checks.push(HealthCheck {
        name: "schema-valid".to_string(),
        ok: app.schema_valid,
        message: if app.schema_valid {
            "database schema is valid".to_string()
        } else {
            "database schema is invalid or could not be validated".to_string()
        },
    });
    let missing_capabilities = missing_capabilities(&app.capabilities);
    checks.push(HealthCheck {
        name: "collector-capabilities".to_string(),
        ok: missing_capabilities.is_empty(),
        message: if missing_capabilities.is_empty() {
            format!("collector capabilities: {}", app.capabilities.join(", "))
        } else {
            format!(
                "collector capability version skew; missing: {}",
                missing_capabilities.join(", ")
            )
        },
    });

    let status = match app.status {
        DiscoveryStatus::Active => "active",
        DiscoveryStatus::Stale => "stale",
    }
    .to_string();
    let ok = checks.iter().all(|check| check.ok);
    AppHealth {
        instance_id: app.instance_id,
        session_id: app.session_id,
        service_name: app.service_name,
        status,
        ok,
        checks,
    }
}

fn missing_capabilities(capabilities: &[String]) -> Vec<String> {
    EXPECTED_CAPABILITIES
        .iter()
        .filter(|expected| {
            !capabilities
                .iter()
                .any(|capability| capability.as_str() == **expected)
        })
        .map(|value| (*value).to_string())
        .collect()
}

fn print_report(report: &HealthReport) {
    println!(
        "Auditaur health: {}",
        if report.ok { "ok" } else { "failed" }
    );
    println!("{}", report.summary);
    if report.apps.is_empty() {
        return;
    }
    println!("APP\tSTATUS\tOK\tCHECKS");
    for app in &report.apps {
        println!(
            "{}\t{}\t{}\t{}",
            table_cell(&app.service_name, 48),
            app.status,
            app.ok,
            app.checks
                .iter()
                .filter(|check| !check.ok)
                .map(|check| check.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}
