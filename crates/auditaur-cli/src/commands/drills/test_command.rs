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
        real::{LaunchSource, RealHost, WindowMode, WindowRequest},
        Reporter, RunnerOptions, Stopped, TestDone,
    },
    select, Filters, Selected, DEFAULT_DRILLS_DIR,
};
use crate::commands::{
    drills::model::{DrillSuite, TestCase},
    workflow,
};

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
    /// `--window`; else `AUDITAUR_DRILL_WINDOW`, then the config, then background.
    pub window: Option<WindowMode>,
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
        keep_last_failed(&mut plan, &workspace, &last_failed_ids(&workspace)?);
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
    let window = resolve_window(
        options.window,
        std::env::var(auditaur_core::drive_bridge::WINDOW_MODE_ENV).ok(),
        config.as_ref().and_then(|config| config.window.as_deref()),
        &options.config,
    )?;
    let cancel = Arc::new(AtomicBool::new(false));
    let mut host = RealHost {
        app,
        source,
        require_frontend: options.require_frontend
            || config
                .as_ref()
                .is_some_and(|config| config.require_frontend),
        ready_timeout: Duration::from_secs(timeout_seconds),
        window,
        window_warned: false,
        workspace_root: workspace.clone(),
        cancel: Arc::clone(&cancel),
        log_dir: None,
        launches: 0,
        failed_launch_log: None,
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
    // Leave nothing running: wait for any app process that was still exiting when its suite ended.
    // The watcher stays up so a cancel request (Ctrl+C or the canvas) still cuts this short.
    let cancelled = || cancel.load(std::sync::atomic::Ordering::SeqCst);
    if let Err(error) =
        crate::process_tree::wait_for_lingering(crate::process_tree::LINGER_TIMEOUT, &cancelled)
    {
        eprintln!("warning: {error:#}");
    }
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

/// The window mode: `--window`, else the environment variable, else `drillTest.window`, else
/// background.
pub(crate) fn resolve_window(
    flag: Option<WindowMode>,
    env: Option<String>,
    config: Option<&str>,
    config_path: &std::path::Path,
) -> Result<WindowRequest> {
    if let Some(mode) = flag {
        return Ok(WindowRequest {
            mode,
            explicit: true,
        });
    }
    let parse = |value: &str, source: String| {
        WindowMode::parse(value)
            .map(|mode| WindowRequest {
                mode,
                explicit: true,
            })
            .ok_or_else(|| anyhow!("{source} is `{value}`; expected `background` or `foreground`"))
    };
    if let Some(value) = env.filter(|value| !value.trim().is_empty()) {
        return parse(
            &value,
            auditaur_core::drive_bridge::WINDOW_MODE_ENV.to_string(),
        );
    }
    if let Some(value) = config {
        return parse(
            value,
            format!("`drillTest.window` in `{}`", config_path.display()),
        );
    }
    Ok(WindowRequest::default())
}

/// Ids of tests that failed or never finished in the newest finished run. Runs still in
/// progress (e.g. a concurrent `drill test`) and unreadable folders are passed over.
/// Keeps the tests `--last-failed` reruns. A serial suite reruns whole, since its later tests
/// need the state earlier ones built.
pub(crate) fn keep_last_failed(
    plan: &mut Vec<Selected<'_>>,
    workspace: &std::path::Path,
    failed: &HashSet<String>,
) {
    for selected in plan.iter_mut() {
        let path = run::workspace_path(workspace, &selected.suite.path);
        let is_failed = |test: &&TestCase| failed.contains(&run::test_id(&path, &test.name));
        if selected.suite.serial {
            if !selected.tests.iter().any(is_failed) {
                selected.tests.clear();
            }
        } else {
            selected.tests.retain(is_failed);
        }
    }
    plan.retain(|selected| !selected.tests.is_empty());
}

pub(crate) fn last_failed_ids(workspace: &std::path::Path) -> Result<HashSet<String>> {
    let root = run::runs_root(&workspace);
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

#[cfg(test)]
mod window_tests {
    use super::{resolve_window, WindowMode, WindowRequest};
    use std::path::Path;

    fn resolve(
        flag: Option<WindowMode>,
        env: Option<&str>,
        config: Option<&str>,
    ) -> anyhow::Result<WindowRequest> {
        resolve_window(
            flag,
            env.map(str::to_owned),
            config,
            Path::new("config.json"),
        )
    }

    #[test]
    fn the_flag_wins_then_the_environment_then_the_config_then_background() {
        let explicit = |mode| WindowRequest {
            mode,
            explicit: true,
        };
        assert_eq!(
            resolve(
                Some(WindowMode::Foreground),
                Some("background"),
                Some("background")
            )
            .unwrap(),
            explicit(WindowMode::Foreground)
        );
        assert_eq!(
            resolve(None, Some("Foreground"), Some("background")).unwrap(),
            explicit(WindowMode::Foreground)
        );
        assert_eq!(
            resolve(None, Some(" "), Some("foreground")).unwrap(),
            explicit(WindowMode::Foreground)
        );
        assert_eq!(
            resolve(None, None, None).unwrap(),
            WindowRequest {
                mode: WindowMode::Background,
                explicit: false
            }
        );
    }

    #[test]
    fn an_unknown_mode_names_where_it_came_from() {
        let error = resolve(None, Some("minimized"), None)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("AUDITAUR_DRILL_WINDOW is `minimized`"),
            "{error}"
        );
        let error = resolve(None, None, Some("hidden")).unwrap_err().to_string();
        assert!(
            error.contains("`drillTest.window` in `config.json`"),
            "{error}"
        );
    }
}
