use std::{fs, path::PathBuf, time::Duration};

use anyhow::{anyhow, Result};
use auditaur_collector::exporter_sqlite::SqliteStore;
use auditaur_core::{discovery::DiscoveryFile, resolve_data_dir};
use serde::Serialize;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

const STALE_AFTER: Duration = Duration::from_secs(30);
const CHURN_BURST_WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredApp {
    pub instance_id: String,
    pub session_id: String,
    pub service_name: String,
    pub service_version: Option<String>,
    pub app_identifier: Option<String>,
    pub pid: u32,
    pub started_at: String,
    pub last_heartbeat_at: String,
    pub heartbeat_age_seconds: Option<i64>,
    pub status: DiscoveryStatus,
    pub capabilities: Vec<String>,
    pub database_path: String,
    pub database_readable: bool,
    pub schema_valid: bool,
    pub discovery_path: String,
    pub stale_reason: Option<String>,
    pub superseded_by_session_id: Option<String>,
    pub seconds_until_next_start: Option<i64>,
    pub churn_session_count: Option<usize>,
    pub churn_window_seconds: Option<i64>,
    pub churn_hint: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryStatus {
    Active,
    Stale,
}

pub fn apps_dir() -> Result<PathBuf> {
    Ok(resolve_data_dir(None)?.join("apps"))
}

pub fn list_apps() -> Result<Vec<DiscoveredApp>> {
    let apps_dir = apps_dir()?;
    if !apps_dir.exists() {
        return Ok(Vec::new());
    }

    let mut apps = Vec::new();
    for entry in fs::read_dir(apps_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let bytes = fs::read(&path)?;
        let discovery: DiscoveryFile = serde_json::from_slice(&bytes)?;
        apps.push(app_from_discovery(discovery, path));
    }
    annotate_session_churn(&mut apps);
    apps.sort_by(|left, right| right.last_heartbeat_at.cmp(&left.last_heartbeat_at));
    Ok(apps)
}

pub fn resolve_db(db: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(db) = db {
        return Ok(db);
    }

    let apps = list_apps()?;
    let usable: Vec<_> = apps
        .iter()
        .filter(|app| app.database_readable && app.schema_valid)
        .collect();
    let active: Vec<_> = usable
        .iter()
        .copied()
        .filter(|app| app.status == DiscoveryStatus::Active)
        .collect();

    match active.as_slice() {
        [app] => Ok(PathBuf::from(&app.database_path)),
        [] if usable.len() == 1 => Ok(PathBuf::from(&usable[0].database_path)),
        [] => Err(anyhow!(
            "No discoverable Auditaur session database found. Run `auditaur apps` or pass --db <path>."
        )),
        _ => Err(anyhow!(
            "Multiple active Auditaur sessions found. Run `auditaur apps` and pass --db <path>."
        )),
    }
}

fn app_from_discovery(discovery: DiscoveryFile, discovery_path: PathBuf) -> DiscoveredApp {
    let heartbeat_age_seconds = heartbeat_age(&discovery.last_heartbeat_at);
    let stale_reason = match heartbeat_age_seconds {
        Some(age) if age > i64::try_from(STALE_AFTER.as_secs()).unwrap_or(30) => {
            Some(format!("last heartbeat was {age}s ago"))
        }
        None => Some("last heartbeat timestamp could not be parsed".to_string()),
        _ => None,
    };
    let status = if stale_reason.is_some() {
        DiscoveryStatus::Stale
    } else {
        DiscoveryStatus::Active
    };
    let database_path = PathBuf::from(&discovery.database_path);
    let database_readable = database_path.is_file();
    let schema_valid = database_readable
        && SqliteStore::open(&database_path)
            .and_then(|store| {
                store.migrate()?;
                store.validate_schema()
            })
            .is_ok();

    DiscoveredApp {
        instance_id: discovery.instance_id,
        session_id: discovery.session_id,
        service_name: discovery.service_name,
        service_version: discovery.service_version,
        app_identifier: discovery.app_identifier,
        pid: discovery.pid,
        started_at: discovery.started_at,
        last_heartbeat_at: discovery.last_heartbeat_at,
        heartbeat_age_seconds,
        status,
        capabilities: discovery.capabilities,
        database_path: database_path.to_string_lossy().to_string(),
        database_readable,
        schema_valid,
        discovery_path: discovery_path.to_string_lossy().to_string(),
        stale_reason,
        superseded_by_session_id: None,
        seconds_until_next_start: None,
        churn_session_count: None,
        churn_window_seconds: None,
        churn_hint: None,
    }
}

fn heartbeat_age(value: &str) -> Option<i64> {
    let heartbeat = parse_timestamp(value)?;
    let age = OffsetDateTime::now_utc() - heartbeat;
    Some(age.whole_seconds().max(0))
}

fn annotate_session_churn(apps: &mut [DiscoveredApp]) {
    let snapshot = apps.to_vec();
    for app in apps
        .iter_mut()
        .filter(|app| app.status == DiscoveryStatus::Stale)
    {
        let Some(started_at) = parse_timestamp(&app.started_at) else {
            continue;
        };
        let mut newer_sessions: Vec<_> = snapshot
            .iter()
            .filter(|candidate| candidate.instance_id != app.instance_id)
            .filter(|candidate| same_app_identity(app, candidate))
            .filter_map(|candidate| {
                let candidate_started_at = parse_timestamp(&candidate.started_at)?;
                (candidate_started_at > started_at).then_some((candidate, candidate_started_at))
            })
            .collect();
        newer_sessions.sort_by_key(|(_, candidate_started_at)| *candidate_started_at);
        let Some((newer, newer_started_at)) = newer_sessions.first().copied() else {
            continue;
        };

        let seconds_until_next_start = (newer_started_at - started_at).whole_seconds().max(0);
        let burst_cutoff = started_at + CHURN_BURST_WINDOW;
        let burst_sessions: Vec<_> = newer_sessions
            .iter()
            .copied()
            .filter(|(_, candidate_started_at)| *candidate_started_at <= burst_cutoff)
            .collect();
        let churn_session_count = (1 + burst_sessions.len()).max(2);
        let churn_window_seconds = burst_sessions
            .last()
            .map(|(_, latest_started_at)| (*latest_started_at - started_at).whole_seconds().max(0))
            .unwrap_or(seconds_until_next_start);
        app.superseded_by_session_id = Some(newer.session_id.clone());
        app.seconds_until_next_start = Some(seconds_until_next_start);
        app.churn_session_count = Some(churn_session_count);
        app.churn_window_seconds = Some(churn_window_seconds);
        app.churn_hint = Some(if churn_session_count >= 3 {
            format!(
                "stale session appears part of a restart burst: {churn_session_count} sessions for this app identity started within {churn_window_seconds}s; likely repeated app restarts or Tauri dev watcher rebuild churn if this followed source edits"
            )
        } else if newer.status == DiscoveryStatus::Active {
            format!(
                "stale session appears superseded by active session {} started {}s later; likely app restart or Tauri dev watcher rebuild if this followed source edits",
                newer.session_id, seconds_until_next_start
            )
        } else {
            format!(
                "stale session has newer session {} started {}s later; inspect session chronology to distinguish app exit from repeated restarts",
                newer.session_id, seconds_until_next_start
            )
        });
    }
}

fn same_app_identity(left: &DiscoveredApp, right: &DiscoveredApp) -> bool {
    match (
        left.app_identifier.as_deref(),
        right.app_identifier.as_deref(),
    ) {
        (Some(left), Some(right)) => left == right,
        _ => left.service_name == right.service_name,
    }
}

fn parse_timestamp(value: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).ok()
}
