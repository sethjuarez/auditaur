//! `auditaur drill test`: run drill suites like a test runner.

use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Context, Result};

use super::{
    default_roots, discover_or_empty, format_ms, format_totals, junit, load, print_issues, run,
    runner::{
        self,
        real::{LaunchSource, RealHost},
        Reporter, RunnerOptions, Stopped, TestDone,
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
    pub trace: runner::TraceMode,
    /// Stop starting tests after this many failures (`-x` is 1).
    pub max_failures: Option<usize>,
    /// Only run tests that failed or did not run in the latest run.
    pub last_failed: bool,
    /// Also write a JUnit XML report here.
    pub junit: Option<PathBuf>,
    pub json: bool,
    /// Launch command from `-- <command>`; wins over the config `start` command.
    pub command: Vec<String>,
}

/// Exit codes: 0 all passed (flaky counts), 1 a test failed or did not run, 2 drill files are
/// invalid, 130 interrupted by Ctrl+C.
pub fn test(options: TestOptions) -> Result<i32> {
    if let Some(value) = misplaced_update_value(&options) {
        bail!(
            "`{value}` was read as a file filter; `--update-snapshots` takes its value after `=`, \
             e.g. `--update-snapshots={value}`"
        );
    }
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
    let mut plan = select(&loaded.suites, &options.filters);
    if options.last_failed {
        let failed = last_failed_ids(&workspace)?;
        for selected in &mut plan {
            let path = run::workspace_path(&workspace, &selected.suite.path);
            selected
                .tests
                .retain(|test| failed.contains(&run::test_id(&path, &test.name)));
        }
        plan.retain(|selected| !selected.tests.is_empty());
        if plan.is_empty() {
            println!("No failed tests in the latest run to rerun.");
            return Ok(0);
        }
    }
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
    let cancel = Arc::new(AtomicBool::new(false));
    let mut host = RealHost {
        app,
        source,
        require_frontend: options.require_frontend
            || config
                .as_ref()
                .is_some_and(|config| config.require_frontend),
        ready_timeout: Duration::from_secs(timeout_seconds),
        workspace_root: workspace.clone(),
        cancel: Arc::clone(&cancel),
        log_dir: None,
        launches: 0,
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
    host.log_dir = Some(writer.dir().to_path_buf());
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
    install_ctrl_c(Arc::clone(&cancel), options.json)?;
    let watcher = CancelWatcher::start(
        writer.dir().join(run::CANCEL_REQUEST_FILE),
        Arc::clone(&cancel),
        options.json,
    );
    let stopped = runner::execute(
        &mut writer,
        &plan,
        &mut host,
        &mut reporter,
        &RunnerOptions {
            retries: options.retries,
            update_snapshots: options.update_snapshots,
            trace: options.trace,
            max_failures: options.max_failures,
            cancel: Some(Arc::clone(&cancel)),
        },
    );
    watcher.stop();
    let stopped = stopped?;
    let report = writer.finish(stopped.is_some())?;
    let suite_failed = report.suites.iter().any(|suite| suite.error.is_some());
    let exit_code = if stopped == Some(Stopped::Cancelled) {
        130
    } else {
        i32::from(report.totals.failed > 0 || report.totals.not_run > 0 || suite_failed)
    };

    if let Some(path) = &options.junit {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, junit::render(&report))
            .with_context(|| format!("failed to write `{}`", path.display()))?;
    }

    if options.json {
        let mut value = serde_json::to_value(&report)?;
        value["dir"] = serde_json::json!(format!("{}/{}", run::RUNS_DIR, report.run_id));
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!();
        match stopped {
            Some(Stopped::Cancelled) => println!("Interrupted."),
            Some(Stopped::MaxFailures) => println!(
                "Stopped after {} (--max-failures); remaining tests did not run.",
                plural(options.max_failures.unwrap_or(1), "failure", "failures")
            ),
            None => {}
        }
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

/// `-u none` parses as bare `-u` (`changed`) plus a `none` file filter, which could silently
/// rewrite baselines; catch that before running.
fn misplaced_update_value(options: &TestOptions) -> Option<&str> {
    use clap::ValueEnum;
    if options.update_snapshots != runner::snapshot::UpdateMode::Changed {
        return None;
    }
    options
        .filters
        .patterns
        .iter()
        .map(String::as_str)
        .find(|pattern| {
            runner::snapshot::UpdateMode::value_variants()
                .iter()
                .filter_map(|mode| mode.to_possible_value())
                .any(|value| value.get_name().eq_ignore_ascii_case(pattern))
        })
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// Ids of tests that failed or never finished in the newest finished run. Runs still in
/// progress (e.g. a concurrent `drill test`) and unreadable folders are passed over.
pub(crate) fn last_failed_ids(workspace: &std::path::Path) -> Result<HashSet<String>> {
    let root = workspace.join(run::RUNS_DIR);
    let ids = run::run_ids(&root).unwrap_or_default();
    let report = ids
        .iter()
        .rev()
        .filter_map(|id| run::read_run(&root.join(id)).ok())
        .find(|report| report.status != run::Status::Running)
        .ok_or_else(|| {
            anyhow!("--last-failed needs a finished previous run; run without it first")
        })?;
    Ok(report
        .suites
        .iter()
        .flat_map(|suite| &suite.tests)
        .filter(|test| matches!(test.outcome, None | Some(run::Outcome::Unexpected)))
        .map(|test| test.id.clone())
        .collect())
}

/// Watches the run folder for [`run::CANCEL_REQUEST_FILE`], which tools such as the MCP
/// `drill_cancel` create to stop a run that has no console to send Ctrl+C to.
struct CancelWatcher {
    done: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl CancelWatcher {
    fn start(path: PathBuf, cancel: Arc<AtomicBool>, quiet: bool) -> Self {
        let done = Arc::new(AtomicBool::new(false));
        let thread = {
            let done = Arc::clone(&done);
            std::thread::spawn(move || {
                while !done.load(Ordering::SeqCst) {
                    if path.exists() {
                        if !cancel.swap(true, Ordering::SeqCst) {
                            let message = "\nStopping: cancel requested; finishing the current step and running teardown";
                            if quiet {
                                eprintln!("{message}");
                            } else {
                                println!("{message}");
                            }
                        }
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
            })
        };
        Self {
            done,
            thread: Some(thread),
        }
    }

    fn stop(mut self) {
        self.done.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// First Ctrl+C stops the run gracefully; a second one exits at once.
fn install_ctrl_c(cancel: Arc<AtomicBool>, quiet: bool) -> Result<()> {
    ctrlc::set_handler(move || {
        if cancel.swap(true, Ordering::SeqCst) {
            std::process::exit(130);
        }
        let message = "\nStopping: finishing the current step and running teardown (Ctrl+C again to quit now)";
        if quiet {
            eprintln!("{message}");
        } else {
            println!("{message}");
        }
    })
    .context("failed to install the Ctrl+C handler")
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
            (Some(run::Outcome::Unexpected), run::Status::Interrupted) => ("x", "interrupted"),
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
            for error in done.error.into_iter().chain(done.other_errors) {
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
