//! `auditaur drill test`: run drill suites like a test runner.

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Result};

use super::{
    default_roots, discover_or_empty, format_ms, format_totals, load, print_issues, run,
    runner::{
        self,
        real::{LaunchSource, RealHost},
        Reporter, RunnerOptions, TestDone,
    },
    select, Filters, DEFAULT_DRILLS_DIR,
};
use crate::commands::{drills::model::DrillSuite, workflow};

const DEFAULT_READY_TIMEOUT_SECONDS: u64 = 180;

pub struct TestOptions {
    pub filters: Filters,
    pub retries: Option<u32>,
    pub app: Option<String>,
    pub config: PathBuf,
    /// Drive an app that is already running instead of launching one.
    pub attach: bool,
    pub require_frontend: bool,
    pub timeout_seconds: Option<u64>,
    pub update_snapshots: runner::snapshot::UpdateMode,
    pub json: bool,
    /// Launch command from `-- <command>`; wins over the config `start` command.
    pub command: Vec<String>,
}

/// Exit codes: 0 all passed (flaky counts), 1 a test failed or did not run, 2 drill files are invalid.
pub fn test(options: TestOptions) -> Result<i32> {
    let workspace = std::env::current_dir()?;
    let files = discover_or_empty(&default_roots())?;
    let loaded = load(&files);
    if !loaded.invalid.is_empty() {
        eprintln!("Fix invalid drill files before running (see `auditaur drill validate`):");
        for file in &loaded.invalid {
            print_issues(file);
        }
        return Ok(2);
    }
    let plan = select(&loaded.suites, &options.filters);
    if plan.iter().all(|selected| selected.tests.is_empty()) {
        eprintln!("No tests found under {DEFAULT_DRILLS_DIR}");
        return Ok(1);
    }

    let config = workflow::load_launch_config(&options.config)?;
    let app = options
        .app
        .clone()
        .or_else(|| config.as_ref().map(|config| config.app.clone()))
        .ok_or_else(|| {
            anyhow!(
                "pass `--app <name>` or create `{}` (run `auditaur init`)",
                options.config.display()
            )
        })?;
    let source = if options.attach {
        LaunchSource::Attach
    } else if !options.command.is_empty() {
        LaunchSource::Command(options.command.clone())
    } else if config
        .as_ref()
        .is_some_and(|config| config.command.is_some())
    {
        LaunchSource::Config(options.config.clone())
    } else {
        bail!(
            "nothing to launch: pass `--attach` to drive a running app, a command after `--`, or add a `start` command to `{}`",
            options.config.display()
        );
    };
    let timeout_seconds = options
        .timeout_seconds
        .or_else(|| config.as_ref().and_then(|config| config.timeout_seconds))
        .unwrap_or(DEFAULT_READY_TIMEOUT_SECONDS);
    let mut host = RealHost {
        app,
        source,
        require_frontend: options.require_frontend
            || config
                .as_ref()
                .is_some_and(|config| config.require_frontend),
        ready_timeout: Duration::from_secs(timeout_seconds),
        workspace_root: workspace.clone(),
    };

    let filters = &options.filters;
    let mut writer = run::RunWriter::create(
        run::RunOptions {
            workspace,
            invocation: run::Invocation {
                args: std::env::args().skip(1).collect(),
                patterns: filters.patterns.clone(),
                grep: filters
                    .grep
                    .as_ref()
                    .map(|regex| regex.as_str().to_string()),
                grep_invert: filters
                    .grep_invert
                    .as_ref()
                    .map(|regex| regex.as_str().to_string()),
                tags: filters.tags.clone(),
            },
            keep: run::DEFAULT_KEEP_RUNS,
        },
        &plan,
    )?;
    let started = Instant::now();
    let mut reporter = LineReporter {
        // Keep stdout clean for the JSON report.
        quiet: options.json,
    };
    let total: usize = plan.iter().map(|selected| selected.tests.len()).sum();
    reporter.say(format!(
        "Running {total} {} from {} {}",
        if total == 1 { "test" } else { "tests" },
        plan.len(),
        if plan.len() == 1 { "suite" } else { "suites" },
    ));
    runner::execute(
        &mut writer,
        &plan,
        &mut host,
        &mut reporter,
        &RunnerOptions {
            retries: options.retries,
            update_snapshots: options.update_snapshots,
        },
    )?;
    let report = writer.finish(false)?;
    let suite_failed = report.suites.iter().any(|suite| suite.error.is_some());
    let exit_code =
        i32::from(report.totals.failed > 0 || report.totals.not_run > 0 || suite_failed);

    if options.json {
        let mut value = serde_json::to_value(&report)?;
        value["dir"] = serde_json::json!(format!("{}/{}", run::RUNS_DIR, report.run_id));
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!();
        println!(
            "{}  ({})",
            format_totals(&report.totals),
            format_ms(Some(started.elapsed().as_millis() as u64))
        );
        println!(
            "Run {} saved to {}/{}; see `auditaur drill show`",
            report.run_id,
            run::RUNS_DIR,
            report.run_id
        );
    }
    Ok(exit_code)
}

/// Playwright `list`-style progress: one line per finished test.
struct LineReporter {
    quiet: bool,
}

impl LineReporter {
    fn say(&self, line: String) {
        if self.quiet {
            eprintln!("{line}");
        } else {
            println!("{line}");
        }
    }
}

impl Reporter for LineReporter {
    fn suite_started(&mut self, suite: &DrillSuite) {
        self.say(String::new());
        self.say(format!(
            "{}  ({})",
            suite.suite,
            super::display_path(&suite.path)
        ));
    }

    fn suite_error(&mut self, suite: &DrillSuite, error: &run::ErrorInfo) {
        self.say(format!(
            "  ! {}: {}",
            suite.suite,
            error.message.replace('\n', "\n    ")
        ));
    }

    fn test_finished(&mut self, done: &TestDone<'_>) {
        let (mark, label) = match (done.outcome, done.status) {
            (None, _) => ("~", "retrying"),
            (Some(run::Outcome::Skipped), _) => ("-", "skipped"),
            (Some(run::Outcome::Flaky), _) => ("~", "flaky"),
            (Some(run::Outcome::Expected), run::Status::Passed) => ("ok", ""),
            (Some(run::Outcome::Expected), _) => ("ok", "expected failure"),
            (Some(run::Outcome::Unexpected), run::Status::TimedOut) => ("x", "timed out"),
            (Some(run::Outcome::Unexpected), run::Status::Passed) => ("x", "passed unexpectedly"),
            (Some(run::Outcome::Unexpected), _) => ("x", ""),
        };
        let retry = if done.attempt > 0 {
            format!(" (retry #{})", done.attempt)
        } else {
            String::new()
        };
        let label = if label.is_empty() {
            String::new()
        } else {
            format!(" [{label}]")
        };
        let time = if done.outcome == Some(run::Outcome::Skipped) {
            String::new()
        } else {
            format!(" ({})", format_ms(Some(done.duration.as_millis() as u64)))
        };
        self.say(format!("  {mark:<2} {}{retry}{label}{time}", done.title));
        let failed = !matches!(
            done.outcome,
            Some(run::Outcome::Expected | run::Outcome::Skipped)
        );
        if failed {
            if let Some(error) = done.error {
                let at = error
                    .step
                    .map(|step| format!("step {}: ", step + 1))
                    .unwrap_or_default();
                self.say(format!(
                    "       {at}{}",
                    error.message.replace('\n', "\n       ")
                ));
            }
        }
    }
}
